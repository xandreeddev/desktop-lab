"""Clipboard persistence contracts using owned temporary files, never a clipboard."""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import clipboard_store as store


class ClipboardStore(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / 'history'
        for name, value in [('ROOT', self.root), ('BYTES', 1024)]:
            setting = patch.object(store, name, value)
            setting.start()
            self.addCleanup(setting.stop)
        clock = patch.object(store.time, 'time', return_value=0)
        clock.start()
        self.addCleanup(clock.stop)

    def bytes(self):
        return {path.name: path.read_bytes() for path in self.root.iterdir() if path.is_file()}

    def history(self, data):
        self.root.mkdir(exist_ok=True)
        path = self.root / 'history.json'
        path.write_bytes(data)
        return path

    def item(self, text='saved text'):
        return {'type': 'text', 'text': text, 'capturedAt': 0,
                'id': hashlib.sha256(b'text\0' + text.encode()).hexdigest()}

    def test_serialized_boundary_is_exact_and_oversize_offer_keeps_previous_entry(self):
        store.capture('text', b'x')
        overhead = (self.root / 'history.json').stat().st_size - 1
        data = b'a' * (store.BYTES - overhead)
        store.capture('text', data)
        self.assertEqual((self.root / 'history.json').stat().st_size, store.BYTES)
        self.assertEqual([item['text'] for item in store.read()], [data.decode()])
        before = self.bytes()
        store.capture('text', data + b'a')
        self.assertEqual(self.bytes(), before)

    def test_sole_unicode_escaped_or_raw_oversize_entry_never_evicts_saved_data(self):
        store.capture('image/png', store.PNG + b'image fixture')
        store.capture('text', b'keep me')
        before = self.bytes()
        for data in [b'"' * 600, b'\0' * 200, ('😀' * 200).encode(), b'x' * 1025]:
            with self.subTest(data_type=data[:4]):
                store.capture('text', data)
                self.assertEqual(self.bytes(), before)

    def test_aggregate_budget_prunes_oldest_and_preserves_exact_text(self):
        for letter in 'abcd':
            store.capture('text', letter.encode() * 200)
        self.assertLessEqual((self.root / 'history.json').stat().st_size, store.BYTES)
        self.assertEqual([item['text'] for item in store.read()],
                         ['d' * 200, 'c' * 200, 'b' * 200])

    def test_count_eviction_dedup_and_image_pruning_are_ordered(self):
        with patch.object(store, 'LIMIT', 2):
            store.capture('image/png', store.PNG + b'image fixture')
            image = Path(store.read()[0]['path'])
            store.capture('text', b'one')
            store.capture('text', b'one')
            self.assertTrue(image.exists())
            store.capture('text', b'two')
            self.assertFalse(image.exists())
            self.assertEqual([item['text'] for item in store.read()], ['two', 'one'])

    def test_sensitive_empty_and_invalid_offers_create_no_storage(self):
        for mime, data, sensitive in [('text', b'secret', True), ('text', b'', False),
                                      ('text', b'\xff', False), ('image/png', b'not png', False)]:
            store.capture(mime, data, sensitive)
        self.assertFalse(self.root.exists())

    def test_read_rejects_invalid_shapes_without_disclosing_or_rewriting_content(self):
        item = self.item()
        invalid = [None, {}, 'private fixture', [None], [{}], [item, item],
                   [item | {'capturedAt': True}], [item | {'capturedAt': -1}],
                   [item | {'capturedAt': 2**63}], [item | {'text': None}],
                   [item | {'text': 'changed'}], [item | {'id': '../outside'}],
                   [item | {'type': 'other'}], [item | {'extra': 'private fixture'}],
                   [item | {'text': '\ud800'}],
                   [{'type': 'image', 'capturedAt': 0, 'id': 'a' * 64,
                     'path': str(self.root.parent / 'outside.png')}]]
        for value in invalid:
            with self.subTest(value=value):
                data = json.dumps(value).encode()
                path = self.history(data)
                with self.assertRaises(store.HistoryError) as error:
                    store.read()
                self.assertEqual(str(error.exception), 'Saved clipboard history could not be read')
                self.assertEqual(path.read_bytes(), data)

    def test_read_rejects_over_count_and_over_bytes_before_json_parsing(self):
        path = self.history(b' ' * (store.BYTES + 1))
        with patch.object(store.json, 'loads') as loads:
            with self.assertRaises(store.HistoryError):
                store.read()
            loads.assert_not_called()
        self.assertEqual(path.stat().st_size, store.BYTES + 1)
        self.history(json.dumps([self.item('one'), self.item('two')]).encode())
        with patch.object(store, 'LIMIT', 1), self.assertRaises(store.HistoryError):
            store.read()

    def test_read_rejects_symlink_fifo_and_invalid_utf8_without_touching_them(self):
        self.root.mkdir()
        path = self.root / 'history.json'
        outside = self.root.parent / 'outside'
        outside.write_bytes(b'[]')
        path.symlink_to(outside)
        with self.assertRaises(store.HistoryError):
            store.read()
        self.assertTrue(path.is_symlink())
        self.assertEqual(outside.read_bytes(), b'[]')
        path.unlink()
        os.mkfifo(path)
        with self.assertRaises(store.HistoryError):
            store.read()
        path.unlink()
        path.write_bytes(b'\xff')
        with self.assertRaises(store.HistoryError):
            store.read()
        self.assertEqual(path.read_bytes(), b'\xff')

    def test_failed_history_read_cannot_capture_text_or_create_an_image(self):
        store.capture('image/png', store.PNG + b'old image')
        self.history(b'{private broken history')
        before = self.bytes()
        for mime, data in [('text', b'new text'), ('image/png', store.PNG + b'new image')]:
            with self.assertRaises(store.HistoryError):
                store.capture(mime, data)
            self.assertEqual(self.bytes(), before)

    def test_failed_history_replace_preserves_old_bytes_and_removes_new_image_and_temp(self):
        store.capture('image/png', store.PNG + b'old image')
        before = self.bytes()
        replace = Path.replace

        def fail_history(path, target):
            if target.name == 'history.json':
                raise OSError('fixture write failure')
            return replace(path, target)

        with patch.object(Path, 'replace', fail_history):
            for mime, data in [('text', b'new text'), ('image/png', store.PNG + b'new image')]:
                with self.assertRaises(OSError):
                    store.capture(mime, data)
                self.assertEqual(self.bytes(), before)
            with self.assertRaises(OSError):
                store.clear()
            self.assertEqual(self.bytes(), before)

    def test_failed_fsync_cleans_temporary_and_preserves_committed_history(self):
        store.capture('text', b'old text')
        before = self.bytes()
        with patch.object(store.os, 'fsync', side_effect=OSError('fixture fsync failure')):
            with self.assertRaises(OSError):
                store.capture('text', b'new text')
        self.assertEqual(self.bytes(), before)

    def test_selected_images_are_bounded_owned_regular_files_and_revalidated(self):
        data = store.PNG + b'fixture'
        store.capture('image/png', data)
        item = store.read()[0]
        path = Path(item['path'])
        self.assertEqual(store.image_data(item), data)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        for replacement in [store.PNG + b'changed', b'not png', store.PNG + b'x' * store.BYTES]:
            path.write_bytes(replacement)
            with self.assertRaises(store.HistoryError):
                store.image_data(item)
        path.unlink()
        with self.assertRaises(store.HistoryError):
            store.image_data(item)
        outside = self.root.parent / 'outside.png'
        outside.write_bytes(data)
        path.symlink_to(outside)
        with self.assertRaises(store.HistoryError):
            store.image_data(item)
        with self.assertRaises(store.HistoryError):
            store.image_data(item | {'path': str(outside)})
        self.assertEqual(outside.read_bytes(), data)

    def test_explicit_clear_recovers_corruption_and_preserves_unowned_files(self):
        store.capture('image/png', store.PNG + b'fixture')
        self.history(b'broken history')
        unrelated = self.root / 'unrelated.png'
        unrelated.write_bytes(b'unrelated')
        self.root.chmod(0o755)
        store.clear()
        self.assertEqual(store.read(), [])
        self.assertEqual(list(self.root.glob('*.png')), [unrelated])
        self.assertEqual(unrelated.read_bytes(), b'unrelated')
        self.assertEqual(self.root.stat().st_mode & 0o777, 0o700)
        self.assertEqual((self.root / 'history.json').stat().st_mode & 0o777, 0o600)
        store.capture('text', 'Recovered 😀\n'.encode())
        self.assertEqual(store.read()[0]['text'], 'Recovered 😀\n')

    def test_simultaneous_watchers_do_not_lose_each_others_entries(self):
        with patch.object(store, 'BYTES', 16 * 1024), ThreadPoolExecutor(max_workers=4) as executor:
            results = [executor.submit(store.capture, 'text', f'fixture {n}'.encode()) for n in range(16)]
            for result in results:
                result.result(timeout=5)
            self.assertEqual({item['text'] for item in store.read()},
                             {f'fixture {n}' for n in range(16)})

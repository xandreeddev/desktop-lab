"""Provider boundaries use synthetic data. Tests never download third-party content."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('catalog_test',ROOT/'scripts/lucent-catalog.py')
catalog=importlib.util.module_from_spec(spec);spec.loader.exec_module(catalog)

class Providers(unittest.TestCase):
    def test_catalog_cannot_fetch_local_or_unrelated_addresses(self):
        for url in ['file:///etc/passwd','http://wallhaven.cc/a','https://127.0.0.1/a','https://wallhaven.cc.evil.test/a','https://user:pass@wallhaven.cc/a','https://wallhaven.cc:8080/a']:
            self.assertFalse(catalog.allowed(url))
        self.assertTrue(catalog.allowed('https://images.alphacoders.com/123/1234567.png'))
        with self.assertRaises(ValueError):catalog.image_extension(b'<html>Challenge</html>')

    def test_wallhaven_filters_non_sfw_and_keeps_original_separate_from_thumbnail(self):
        entry={'id':'abc123','purity':'sfw','path':'https://w.wallhaven.cc/ab/wallhaven-abc123.jpg','thumbs':{'large':'https://th.wallhaven.cc/lg/ab/abc123.jpg'},'resolution':'1920x1080'}
        page=catalog.wallhaven({'data':[entry,dict(entry,purity='nsfw')],'meta':{'last_page':2}},1)
        self.assertEqual(len(page['items']),1);self.assertTrue(page['has_more'])
        self.assertNotEqual(page['items'][0]['image_url'],page['items'][0]['thumbnail_url'])

    def test_alpha_grid_and_paginated_cards_parse_without_executing_scripts(self):
        for attributes in ['class="thumb" src=', 'src=', 'data-src=']:
            html=f'''<a href="https://wall.alphacoders.com/big.php?i=1234567" title="Synthetic mountain"><img {attributes}"https://images.alphacoders.com/123/thumb-350-1234567.webp"></a><span onclick="downloadContentModal('images', 1234567, 'png', 789); return false;"></span>'''
            page=catalog.alpha(html,2)
            self.assertEqual(page['items'][0]['image_url'],'https://images.alphacoders.com/123/1234567.png')
            self.assertEqual(page['page'],2)
        old=html.replace('1234567','238870').replace('/123/','/238/')
        self.assertEqual(catalog.alpha(old,1)['items'][0]['image_url'],'https://images.alphacoders.com/238/238870.png')
        self.assertEqual(catalog.alpha('<html>No results</html>',1)['items'],[])
        with self.assertRaises(ValueError):catalog.alpha('<title>Just a moment</title>',1)

    def test_download_caches_bytes_preserves_source_and_rejects_non_images(self):
        item={'provider':'wallhaven','id':'abc123','title':'Fixture','page_url':'https://wallhaven.cc/w/abc123','thumbnail_url':'https://th.wallhaven.cc/lg/ab/abc123.jpg','image_url':'https://w.wallhaven.cc/ab/wallhaven-abc123.jpg'}
        with tempfile.TemporaryDirectory() as directory,patch.object(catalog,'LIBRARY',Path(directory)):
            with patch.object(catalog,'fetch',return_value=b'<html>error</html>'):
                with self.assertRaises(ValueError):catalog.download(item,False)
                self.assertEqual(list(Path(directory).iterdir()),[])
            with patch.object(catalog,'fetch',return_value=b'\xff\xd8\xfffixture') as fetch:
                first=catalog.download(item,False);second=catalog.download(item,False)
                self.assertEqual(first,second);fetch.assert_called_once()
                self.assertEqual(json.loads(Path(first['path']).with_suffix('.source.json').read_text())['page_url'],item['page_url'])

    def test_alpha_pagination_preserves_canonical_search_route(self):
        first=b'<link rel="canonical" href="https://alphacoders.com/landscape">'
        with patch.object(catalog,'fetch',side_effect=[first,b'<html>No results</html>']) as fetch:
            page=catalog.search('alpha-coders','landscape',2)
            self.assertEqual(page['page'],2)
            self.assertEqual(fetch.call_args.args[0],'https://alphacoders.com/landscape?page=2')

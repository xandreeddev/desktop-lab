"""Preserve a selected immutable wallpaper across test-induced cache pruning.

The desktop's cache policy remains unchanged. This is test tooling: checkpoint
only the selected owned snapshot, restore it before the normal IPC rollback,
and remove the checkpoint only after the caller verifies the original desktop.
"""
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tempfile


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def owned_directory(state, asset):
    image = Path(asset['path'])
    assets = (state / 'wallpaper-assets').resolve()
    if image.parent.parent != assets:
        return None  # Legacy/user originals are outside the store's pruning policy.
    directory = image.parent
    if not directory.name.startswith('asset-') or directory.is_symlink() or image.is_symlink():
        raise ValueError('Invalid owned wallpaper snapshot')
    if image.resolve() != image:
        raise ValueError('Wallpaper snapshot must use its canonical path')
    return directory


def create_checkpoint(state, asset, scratch=None):
    directory = owned_directory(state, asset)
    if directory is None:
        return None
    image = Path(asset['path'])
    metadata = directory / 'source.json'
    if metadata.is_symlink() or json.loads(metadata.read_text()) != asset:
        raise ValueError('Selected wallpaper metadata changed')
    checkpoint = Path(tempfile.mkdtemp(prefix='lucent-wallpaper-checkpoint-', dir=scratch))
    try:
        shutil.copy2(image, checkpoint / 'image')
        (checkpoint / 'manifest.json').write_text(json.dumps({'asset': asset, 'sha256': digest(image)}))
        if digest(checkpoint / 'image') != digest(image):
            raise ValueError('Selected wallpaper changed during checkpoint')
    except BaseException:
        shutil.rmtree(checkpoint)
        raise
    return str(checkpoint)


def read_checkpoint(state, checkpoint):
    checkpoint = Path(checkpoint)
    if checkpoint.is_symlink() or not checkpoint.name.startswith('lucent-wallpaper-checkpoint-'):
        raise ValueError('Invalid wallpaper checkpoint directory')
    manifest = json.loads((checkpoint / 'manifest.json').read_text())
    if owned_directory(state, manifest['asset']) is None:
        raise ValueError('Checkpoint does not belong to the wallpaper store')
    if (checkpoint / 'image').is_symlink() or digest(checkpoint / 'image') != manifest['sha256']:
        raise ValueError('Wallpaper checkpoint content changed')
    return checkpoint, manifest


def restore_checkpoint(state, checkpoint):
    if checkpoint is None:
        return
    checkpoint, manifest = read_checkpoint(state, checkpoint)
    asset = manifest['asset']
    image = Path(asset['path'])
    directory = image.parent
    if directory.exists():
        # Preserve any independently changed files instead of overwriting them.
        if json.loads((directory / 'source.json').read_text()) != asset or digest(image) != manifest['sha256']:
            raise ValueError('Rollback snapshot was independently changed; checkpoint retained')
        return
    # Restore only the owned snapshot; the caller selects it via the public IPC.
    directory.mkdir(mode=0o700)
    try:
        shutil.copy2(checkpoint / 'image', image)
        (directory / 'source.json').write_text(json.dumps(asset))
    except BaseException:
        shutil.rmtree(directory)
        raise


def discard_checkpoint(state, checkpoint):
    if checkpoint is not None:
        checkpoint, _ = read_checkpoint(state, checkpoint)
        shutil.rmtree(checkpoint)


class GuestWallpaperCheckpoint:
    def __init__(self, remote, asset):
        self.remote = remote
        self.source = Path(__file__).read_text()
        self.path = json.loads(remote('python3', '-c', self.source, 'create', json.dumps(asset)))

    def restore(self):
        self.remote('python3', '-c', self.source, 'restore', json.dumps(self.path))

    def discard(self):
        self.remote('python3', '-c', self.source, 'discard', json.dumps(self.path))


def main():
    state = Path.home() / '.local/state/lucent'
    action, value = sys.argv[1:]
    value = json.loads(value)
    if action == 'create':
        print(json.dumps(create_checkpoint(state, value)))
    elif action == 'restore':
        restore_checkpoint(state, value)
    elif action == 'discard':
        discard_checkpoint(state, value)
    else:
        raise ValueError('Unknown checkpoint operation')


if __name__ == '__main__':
    main()

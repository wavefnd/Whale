"""Keep one bounded original and replay manifest before entering native code."""
import hashlib
import json
import os
from pathlib import Path
import tempfile


def atomic_write(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    handle, temporary = tempfile.mkstemp(prefix='.' + path.name + '-', dir=path.parent)
    try:
        with os.fdopen(handle, 'wb') as stream:
            stream.write(data)
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


class Checkpoint:
    def __init__(self, directory, name, replay_command, max_bytes=64 << 20):
        self.directory = Path(directory)
        self.name = name
        self.replay_command = list(map(str, replay_command))
        self.max_bytes = max_bytes
        self.input = self.directory / (name + '.current')
        self.manifest = self.directory / (name + '.json')
        self.started = False
        self.digest = None

    def before(self, original, **metadata):
        if len(original) > self.max_bytes:
            raise ValueError('checkpoint input exceeds its byte ceiling')
        if not self.started:
            (self.directory / (self.name + '.completed.json')).unlink(missing_ok=True)
            self.started = True
        # The manifest is the commit point. Its original stays immutable until
        # a new manifest is published, including interruption between the writes.
        previous = self.input if self.input.is_file() else None
        if previous is None and self.manifest.is_file():
            saved = Path(json.loads(self.manifest.read_text())['input'])
            suffix = saved.name.removeprefix(self.name + '.current-')
            if saved.parent == self.directory and len(suffix) == 64 and all(c in '0123456789abcdef' for c in suffix):
                previous = saved
        digest = hashlib.sha256(original).hexdigest()
        candidate = self.directory / (self.name + '.current-' + digest)
        atomic_write(candidate, original)
        record = dict(metadata, state='active', input=str(candidate),
                      bytes=len(original), sha256=digest,
                      replay=[*self.replay_command, '--replay', str(candidate)])
        try:
            atomic_write(self.manifest, (json.dumps(record, indent=2) + '\n').encode())
        except BaseException:
            if candidate != previous:
                candidate.unlink(missing_ok=True)
            raise
        self.input = candidate
        self.digest = digest
        if previous is not None and previous != candidate:
            previous.unlink(missing_ok=True)

    def finish(self, **metadata):
        atomic_write(self.directory / (self.name + '.completed.json'),
                     (json.dumps(dict(metadata, state='completed', input_sha256=self.digest), indent=2) + '\n').encode())

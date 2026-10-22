#!/usr/bin/env python3
"""Opt-in native shell development: build, then restart only after success."""
import argparse
import json
import math
import os
from pathlib import Path
import selectors
import sys
import time
from lucent_dev import build, deploy
from lucent_dev.model import DevError, load_bundle, mutation_lease, private_directory
from lucent_dev.watch import Debounce, Watcher, source_digest


def paths(args):
    root = args.root.resolve()
    if not (root / 'lucent/Cargo.toml').is_file():
        raise DevError('--root must contain lucent/Cargo.toml')
    target = args.target_dir or Path(os.environ.get('CARGO_TARGET_DIR', root / 'lucent/target'))
    if not target.is_absolute():
        target = root / target
    cache = private_directory(Path(os.environ.get('XDG_CACHE_HOME', Path.home() / '.cache')) / 'lucent-dev')
    output = private_directory(cache / 'bundles')
    return root, target.resolve(), output


def fresh(result, root, target, output):
    return source_digest(root, (target, output)) == result['inputs']


def once(args, apply):
    if apply:
        try:
            deploy.preflight()
        except deploy.Deferred as error:
            print('Deployment will wait: ' + str(error), flush=True)
    root, target, output = paths(args)
    job = build.BuildJob(root, target, args.profile, output)
    try:
        result = job.finish()
        print('Successful bundle: ' + result['bundle'], flush=True)
        if apply:
            if not fresh(result, root, target, output):
                raise DevError('Source changed after build; the running shell was not changed')
            print(json.dumps(deploy.apply_bundle(Path(result['bundle']), ready_timeout=args.ready_timeout)))
    except BaseException:
        if job.process.poll() is None:
            job.cancel()
        raise
    finally:
        job.close()


def watch(args):
    root, target, output = paths(args)
    try:
        deploy.preflight()
    except deploy.Deferred as error:
        print('Deployment will wait: ' + str(error), flush=True)
    queue = Debounce(args.debounce)
    pending = job = None
    retry_at = 0.
    with Watcher(root, (target, output)) as watcher, selectors.DefaultSelector() as selector:
        selector.register(watcher, selectors.EVENT_READ, 'source')
        try:
            while True:
                now = time.monotonic()
                deadlines = []
                if job is None:
                    if queue.dirty:
                        deadlines.append(queue.deadline)
                    elif pending:
                        deadlines.append(retry_at)
                timeout = max(0., min(deadlines) - now) if deadlines else None
                events = selector.select(timeout)
                if any(key.data == 'source' for key, _ in events) and watcher.read():
                    queue.changed()
                    pending = None
                if job is not None and any(key.data == 'build' for key, _ in events):
                    selector.unregister(job)
                    if watcher.read():
                        queue.changed()
                        pending = None
                    try:
                        result = job.finish()
                        if fresh(result, root, target, output):
                            pending = result
                            retry_at = time.monotonic()
                            queue.dirty = False
                            print('Successful bundle: ' + result['bundle'], flush=True)
                        else:
                            queue.changed()
                            print('Newer source superseded this build', flush=True)
                    except (DevError, OSError, ValueError) as error:
                        print('Build not applied: ' + str(error), file=sys.stderr, flush=True)
                    finally:
                        job.close()
                        job = None
                now = time.monotonic()
                if job is None and queue.ready(now):
                    queue.begin()
                    pending = None
                    job = build.BuildJob(root, target, args.profile, output)
                    selector.register(job, selectors.EVENT_READ, 'build')
                    print('Building latest source in ' + job.unit, flush=True)
                elif job is None and pending and not queue.dirty and now >= retry_at:
                    if not fresh(pending, root, target, output):
                        pending = None
                        queue.changed()
                        continue
                    try:
                        result = deploy.apply_bundle(Path(pending['bundle']), ready_timeout=args.ready_timeout)
                        print(json.dumps(result), flush=True)
                        pending = None
                    except deploy.Deferred as error:
                        print('Successful bundle retained; waiting: ' + str(error), flush=True)
                        retry_at = time.monotonic() + 2.
                    except deploy.Interrupted:
                        raise
                    except deploy.DeployError as error:
                        print('Deployment stopped: ' + str(error), file=sys.stderr, flush=True)
                        pending = None
        finally:
            if job is not None:
                job.cancel()
                job.close()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', nargs='?', default='watch', choices=('watch', 'once', 'build', 'apply', 'doctor'))
    parser.add_argument('bundle', nargs='?', type=Path)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--target-dir', type=Path)
    parser.add_argument('--profile', choices=('release', 'debug'), default='release')
    parser.add_argument('--debounce', type=float, default=.35)
    parser.add_argument('--ready-timeout', type=float, default=45.)
    args = parser.parse_args(argv)
    if not math.isfinite(args.debounce) or not .05 <= args.debounce <= 5:
        parser.error('--debounce must be between 0.05 and 5 seconds')
    if not math.isfinite(args.ready_timeout) or not 35 <= args.ready_timeout <= 120:
        parser.error('--ready-timeout must be between 35 and 120 seconds')
    if bool(args.bundle) != (args.command == 'apply'):
        parser.error('Only apply takes a bundle path, and it requires one')
    try:
        if args.command == 'doctor':
            print(json.dumps(deploy.preflight()))
            return 0
        with mutation_lease():
            if args.command == 'apply':
                load_bundle(args.bundle)
                print(json.dumps(deploy.apply_bundle(args.bundle, ready_timeout=args.ready_timeout)))
            elif args.command == 'watch':
                watch(args)
            else:
                once(args, apply=args.command == 'once')
        return 0
    except deploy.Interrupted as error:
        print('lucent-dev: ' + str(error), file=sys.stderr)
        return 128 + error.signum
    except KeyboardInterrupt:
        print('Development command stopped; no further updates will be applied', file=sys.stderr)
        return 130
    except (DevError, deploy.DeployError, OSError, ValueError) as error:
        print('lucent-dev: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())

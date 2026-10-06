"""Bounded native output observations while a CLI waits for input EOF."""
import contextlib
import errno
import fcntl
import functools
import hashlib
import os
import pty
import resource
import select
import signal
import stat
import subprocess
import sys
import tempfile
import termios
import time
import tty
from pathlib import Path


def native_fault_identity():
    """Identify explicitly built native test helpers, without changing a parent."""
    library = Path(os.environ['FASTMASH_TEST_FULL_OUTPUT_LIBRARY']).resolve(strict=True)
    probe = Path(os.environ.get('FASTMASH_TEST_FULL_OUTPUT_PROBE',
                                library.with_name('full-output-probe'))).resolve(strict=True)
    return dict(library=str(library), probe=str(probe),
                library_sha256=hashlib.sha256(library.read_bytes()).hexdigest(),
                probe_sha256=hashlib.sha256(probe.read_bytes()).hexdigest())


@functools.lru_cache(maxsize=None)
def _calibrate_native_full(binary, library, probe, binary_hash, library_hash, probe_hash):
    # Hashes bind the cache to the executable and helper bytes even when a path
    # is reused. Only direct test executables receive dyld's environment.
    environment = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
    def run(program, args, env, stdout=None):
        argv0 = 'fastmash' if program == binary else str(program)
        return subprocess.run([argv0] + args, executable=str(program), env=env, input=b'',
                              stdout=stdout or subprocess.PIPE,
                              stderr=subprocess.PIPE, timeout=5, check=False)
    for fd, loaded in ((0, False), (0, True), (1, True), (2, True)):
        env = environment.copy()
        if loaded:
            env['DYLD_INSERT_LIBRARIES'] = library
        if fd:
            env.update(DYLD_INSERT_LIBRARIES=library, FASTMASH_TEST_FULL_FD=str(fd))
        result = run(probe, [str(fd)], env)
        expected = (b'' if fd == 1 else b'stdout\n', b'' if fd == 2 else b'stderr\n')
        if result.returncode != 0 or (result.stdout, result.stderr) != expected:
            raise RuntimeError(f'Native output-fault probe failed for fd {fd}: {result!r}')
    version = run(binary, ['--version'], environment)
    if version.returncode or not version.stdout.startswith(b'fastmash '):
        raise RuntimeError('Native fault target is not the direct Fastmash executable')
    with open('/dev/null', 'wb') as null:
        ordinary = run(binary, ['--version'], environment, null)
        if ordinary.returncode or ordinary.stderr:
            raise RuntimeError('Ordinary /dev/null output calibration failed')
        full_env = dict(environment, DYLD_INSERT_LIBRARIES=library, FASTMASH_TEST_FULL_FD='1')
        full = run(binary, ['--version'], full_env, null)
        if full.returncode != 1 or full.stderr != b'fastmash: write error: No space left on device\n':
            raise RuntimeError(f'Native full stdout calibration failed: {full!r}')
    diagnostic = run(binary, ['--bad'], environment)
    delegated = run(binary, ['--bad'], full_env)
    if diagnostic.returncode != 1 or not diagnostic.stderr or (
            delegated.returncode, delegated.stdout, delegated.stderr) != (
            diagnostic.returncode, diagnostic.stdout, diagnostic.stderr):
        raise RuntimeError('Unselected native stderr write did not delegate exactly')
    error_env = dict(environment, DYLD_INSERT_LIBRARIES=library, FASTMASH_TEST_FULL_FD='2')
    failed = run(binary, ['--bad'], error_env)
    if failed.returncode != 1 or failed.stdout or failed.stderr:
        raise RuntimeError('Native full stderr calibration failed')
    delegated = run(binary, ['--version'], error_env)
    if (delegated.returncode, delegated.stdout, delegated.stderr) != (
            version.returncode, version.stdout, version.stderr):
        raise RuntimeError('Unselected native stdout write did not delegate exactly')


def native_full_environment(binary, environment, fd=1):
    """Fail closed, then attach a selected stream fault to one native child."""
    if sys.platform != 'darwin' or fd not in (1, 2):
        raise ValueError('Native full-output injection requires Darwin fd 1 or 2')
    identity = native_fault_identity()
    binary = Path(binary).resolve(strict=True)
    _calibrate_native_full(str(binary), identity['library'], identity['probe'],
                           hashlib.sha256(binary.read_bytes()).hexdigest(),
                           identity['library_sha256'], identity['probe_sha256'])
    return dict(environment, DYLD_INSERT_LIBRARIES=identity['library'],
                FASTMASH_TEST_FULL_FD=str(fd))


@functools.lru_cache(maxsize=None)
def _calibrate_native_read(binary, library, probe, binary_hash, library_hash, probe_hash):
    _calibrate_native_full(binary, library, probe, binary_hash, library_hash, probe_hash)
    environment = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
    data = b'complete\npartial'
    def run(program, args, env):
        argv0 = 'fastmash' if program == binary else str(program)
        return subprocess.run([argv0] + args, executable=str(program), env=env, input=data,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              timeout=5, check=False)
    for loaded, selector in ((False, None), (True, None), (True, ''),
                             (True, '0'), (True, '11'), (True, '1')):
        env = environment.copy()
        if loaded:
            env['DYLD_INSERT_LIBRARIES'] = library
        if selector is not None:
            env['FASTMASH_TEST_READ_ERROR'] = selector
        result = run(probe, ['read', 'error' if selector == '1' else 'eof'], env)
        if result.returncode or result.stdout != data or result.stderr:
            raise RuntimeError(f'Native read-fault probe failed for {loaded}/{selector!r}: {result!r}')
    selected = dict(environment, DYLD_INSERT_LIBRARIES=library, FASTMASH_TEST_READ_ERROR='1')
    with open('/dev/null', 'wb') as write_only:
        access = subprocess.run([probe, 'read-access'], env=selected, stdin=write_only,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                timeout=5, check=False)
    if access.returncode or access.stdout or access.stderr:
        raise RuntimeError('Native selected read error did not preserve EBADF')
    ordinary = run(binary, ['reverse'], environment)
    disabled = run(binary, ['reverse'], dict(environment, DYLD_INSERT_LIBRARIES=library))
    if ordinary.returncode or ordinary.stdout != b'complete\npartial\n' or ordinary.stderr or (
            disabled.returncode, disabled.stdout, disabled.stderr) != (
            ordinary.returncode, ordinary.stdout, ordinary.stderr):
        raise RuntimeError('Native ordinary or library-disabled EOF changed')
    failed = run(binary, ['reverse'], selected)
    if (failed.returncode, failed.stdout, failed.stderr) != (
            1, b'complete\n', b'fastmash: read error: Input/output error\n'):
        raise RuntimeError(f'Native candidate late EIO calibration failed: {failed!r}')
    combined = run(binary, ['reverse'], dict(selected, FASTMASH_TEST_FULL_FD='1'))
    if (combined.returncode, combined.stdout, combined.stderr) != (
            1, b'', b'fastmash: read error: Input/output error\nfastmash: write error\n'):
        raise RuntimeError(f'Native combined read/output fault calibration failed: {combined!r}')


def native_read_error_environment(binary, environment):
    """Calibrate native late EIO and arm only the direct candidate's fd 0."""
    if sys.platform != 'darwin':
        raise ValueError('Native read-error injection requires Darwin')
    identity = native_fault_identity()
    binary = Path(binary).resolve(strict=True)
    _calibrate_native_read(str(binary), identity['library'], identity['probe'],
                           hashlib.sha256(binary.read_bytes()).hexdigest(),
                           identity['library_sha256'], identity['probe_sha256'])
    return dict(environment, DYLD_INSERT_LIBRARIES=identity['library'],
                FASTMASH_TEST_READ_ERROR='1')


def admit_capture(fd):
    info = os.fstat(fd)
    if (not stat.S_ISREG(info.st_mode)
            or (info.st_blksize != 4096 if sys.platform == 'linux' else info.st_blksize <= 0)
            or os.isatty(fd) or fcntl.fcntl(fd, fcntl.F_GETFL) & os.O_ACCMODE == os.O_RDONLY):
        raise ValueError('Bounded capture preconditions failed')


def admit_destination(fd, transport):
    info = os.fstat(fd)
    if transport == 'normal':
        admit_capture(fd)
    elif transport in ('pipe', 'closed-pipe'):
        if (not stat.S_ISFIFO(info.st_mode) or os.isatty(fd)
                or (info.st_blksize != 4096 if sys.platform == 'linux' else info.st_blksize <= 0)):
            raise ValueError('Bounded pipe preconditions failed')
    elif transport == 'full':
        device = os.makedev(1, 7) if sys.platform == 'linux' else os.stat('/dev/null').st_rdev
        if (not stat.S_ISCHR(info.st_mode) or info.st_rdev != device
                or fcntl.fcntl(fd, fcntl.F_GETFL) & os.O_ACCMODE == os.O_RDONLY):
            raise ValueError('Bounded full-device identity differs')
    elif transport == 'pty':
        attrs = termios.tcgetattr(fd)
        if (not stat.S_ISCHR(info.st_mode) or not os.isatty(fd)
                or (info.st_blksize != 1024 if sys.platform == 'linux' else info.st_blksize <= 0)
                or attrs[0] & (termios.IGNBRK | termios.BRKINT | termios.PARMRK | termios.ICRNL
                               | termios.INLCR | termios.IGNCR | termios.ISTRIP | termios.IXON)
                or attrs[1] & termios.OPOST
                or attrs[3] & (termios.ECHO | termios.ICANON | termios.IEXTEN | termios.ISIG)
                or attrs[2] & (termios.CSIZE | termios.PARENB) != termios.CS8
                or attrs[6][termios.VMIN] != 1 or attrs[6][termios.VTIME] != 0):
            raise ValueError('Bounded raw PTY preconditions failed')
    else:
        raise ValueError('Unreviewed bounded output transport')


def invoke(case, binary, directory, argv, env, cap, *, bounded=False):
    data = bytes.fromhex(case['input_hex'])
    if not data:
        raise ValueError('Held-input observation needs a record')
    with contextlib.ExitStack() as stack:
        output_file = stack.enter_context(tempfile.TemporaryFile(dir=directory))
        error_file = stack.enter_context(tempfile.TemporaryFile(dir=directory))
        streaming = case['io'] in ('pipe', 'pty')
        read_fd = None
        if streaming:
            read_fd, out = pty.openpty() if case['io'] == 'pty' else os.pipe()
            stack.callback(os.close, read_fd)
            if case['io'] == 'pty':
                tty.setraw(out)
        else:
            out = output_file.fileno()
        facts = dict(isatty=os.isatty(out), block_size=os.fstat(out).st_blksize)
        def limits():
            resource.setrlimit(resource.RLIMIT_FSIZE, (cap, cap))
            if bounded:
                resource.setrlimit(resource.RLIMIT_CPU, (10, 10))
        started = time.monotonic()
        try:
            if bounded:
                admit_capture(output_file.fileno())
                admit_capture(error_file.fileno())
                admit_destination(out, case['io'])
            proc = subprocess.Popen(argv, executable=str(binary), stdin=subprocess.PIPE,
                stdout=out, stderr=error_file, cwd=directory, env=env,
                start_new_session=True, restore_signals=True,
                preexec_fn=limits)
        finally:
            if streaming:
                os.close(out)
        deadline = (started if bounded else time.monotonic()) + 8
        output = bytearray()
        held, observation_end = None, None
        reader_open = streaming
        completed = False
        try:
            if streaming:
                os.set_blocking(read_fd, False)
            os.set_blocking(proc.stdin.fileno(), False)
            pending = memoryview(data)
            while reader_open or proc.poll() is None:
                now = time.monotonic()
                if now >= deadline:
                    raise RuntimeError('Held-input process deadline exceeded')
                readable, writable, _ = select.select(
                    [read_fd] if reader_open else [], [proc.stdin] if pending else [], [],
                    min(0.05, deadline - now))
                if readable:
                    try:
                        chunk = os.read(read_fd, min(65536, cap + 1 - len(output)))
                    except BlockingIOError:
                        chunk = None
                    except OSError as error:
                        if case['io'] != 'pty' or error.errno != errno.EIO:
                            raise
                        chunk = b''
                    if chunk == b'':
                        reader_open = False
                    elif chunk:
                        output.extend(chunk)
                        if (len(output) >= cap) if bounded else (len(output) > cap):
                            raise RuntimeError('Held-input output exceeds capture bound')
                if writable:
                    try:
                        pending = pending[os.write(proc.stdin.fileno(), pending):]
                    except BlockingIOError:
                        pass
                    if not pending:
                        observation_end = time.monotonic() + 2
                if observation_end is not None and time.monotonic() >= observation_end and held is None:
                    snapshot = bytes(output) if streaming else os.pread(out, cap + 1, 0)
                    if (len(snapshot) >= cap) if bounded else (len(snapshot) > cap):
                        raise RuntimeError('Held-input snapshot exceeds capture bound')
                    held = dict(bytes=len(snapshot), alive=proc.poll() is None)
                    if streaming:
                        held['stdout_hex'] = snapshot.hex()
                    proc.stdin.close()
                    proc.stdin = None
            proc.wait(timeout=max(0.001, deadline - time.monotonic()))
            completed = True
        finally:
            if not completed:
                try:
                    os.killpg(proc.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            try:
                proc.wait(timeout=0.5 if bounded else 2)
            finally:
                if proc.stdin is not None:
                    proc.stdin.close()
        if not streaming:
            output = os.pread(out, cap + 1, 0)
        error = os.pread(error_file.fileno(), cap + 1, 0)
        if (len(output) >= cap or len(error) >= cap) if bounded else (len(output) > cap or len(error) > cap):
            raise RuntimeError('Held-input output exceeds capture bound')
        return dict(argv=[os.fsdecode(a) for a in argv], executable=str(binary), env=env,
                    process_group=proc.pid,
                    returncode=proc.returncode, timed_out=False, stdout_hex=output.hex(),
                    stderr_hex=error.hex(), held=held, destination=facts,
                    cleanup_confirmed=True, overflow=False)

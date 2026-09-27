"""Bounded output observations while a CLI waits for input EOF (Linux only)."""
import contextlib
import errno
import fcntl
import os
import pty
import resource
import select
import signal
import stat
import subprocess
import tempfile
import termios
import time
import tty


def admit_capture(fd):
    info = os.fstat(fd)
    if (not stat.S_ISREG(info.st_mode) or info.st_blksize != 4096
            or os.isatty(fd) or fcntl.fcntl(fd, fcntl.F_GETFL) & os.O_ACCMODE == os.O_RDONLY):
        raise ValueError('Bounded capture preconditions failed')


def admit_destination(fd, transport):
    info = os.fstat(fd)
    if transport == 'normal':
        admit_capture(fd)
    elif transport in ('pipe', 'closed-pipe'):
        if not stat.S_ISFIFO(info.st_mode) or info.st_blksize != 4096 or os.isatty(fd):
            raise ValueError('Bounded pipe preconditions failed')
    elif transport == 'full':
        if not stat.S_ISCHR(info.st_mode) or info.st_rdev != os.makedev(1, 7):
            raise ValueError('Bounded full-device identity differs')
    elif transport == 'pty':
        attrs = termios.tcgetattr(fd)
        if (not stat.S_ISCHR(info.st_mode) or not os.isatty(fd) or info.st_blksize != 1024
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

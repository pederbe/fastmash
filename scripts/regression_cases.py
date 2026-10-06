"""Run one retained regression case against a fastmash binary and compare it.

Used by scripts/check_regressions.py, and shared so that every runner invokes
and compares cases the same way. Each case names the argv[0] to invoke
(`name`); diagnostics print it.
"""
import contextlib
import os
import re
import signal
import subprocess
import sys
import tempfile
import time

import cli_streams

CAP = 524288


def invoke(case, binary, directory, *, bounded=False):
    if bounded:
        observed = bool(case.get('observe_held'))
        if (case['io'] not in (('normal', 'pipe', 'pty') if observed else
                              ('pipe', 'directory', 'full', 'closed-pipe'))
                or (case.get('hold_stdin') and
                    (observed or case['io'] != 'pipe' or case['input_hex']))):
            raise ValueError('Unreviewed bounded transport combination')
    if case['io'] == 'read-error':
        import cli_read_error
        return cli_read_error.invoke(case, binary, directory, CAP)
    # Preserve the reviewed process boundary, including deliberately unusual I/O.
    import resource
    if signal.pthread_sigmask(signal.SIG_BLOCK, set()):
        raise RuntimeError('Run from a shell with an unblocked signal mask')
    env = {'PATH': '/usr/bin:/bin', 'LC_ALL': case['locale'], 'TZ': 'UTC'}
    if 'posixly_correct' in case:
        env['POSIXLY_CORRECT'] = case['posixly_correct']
    args = [bytes.fromhex(a) for a in case['args_hex']] if 'args_hex' in case else case['args']
    argv = [case['name']] + args
    if case.get('observe_held'):
        return cli_streams.invoke(case, binary, directory, argv, env,
                                  CAP // 2 if bounded else CAP, bounded=bounded)
    with contextlib.ExitStack() as stack:
        stdout_file = stack.enter_context(tempfile.TemporaryFile(dir=directory))
        stderr_file = stack.enter_context(tempfile.TemporaryFile(dir=directory))
        if bounded:
            import fcntl
            import stat
            for capture in (stdout_file, stderr_file):
                descriptor = capture.fileno()
                info = os.fstat(descriptor)
                if (not stat.S_ISREG(info.st_mode) or info.st_blksize != 4096
                        or os.isatty(descriptor)
                        or fcntl.fcntl(descriptor, fcntl.F_GETFL) & os.O_ACCMODE == os.O_RDONLY):
                    raise ValueError('Bounded capture preconditions failed')
        inp, out = subprocess.PIPE, stdout_file
        data = bytes.fromhex(case['input_hex'])
        if case['io'] == 'directory':
            inp = os.open(str(directory), os.O_RDONLY | os.O_DIRECTORY)
            stack.callback(os.close, inp)
            data = None
        if case['io'] == 'file':
            # Standard input as a regular file, which input from a file takes
            # (hash grouping, for one) and a pipe does not.
            source = stack.enter_context(tempfile.TemporaryFile(dir=directory))
            source.write(data)
            source.seek(0)
            inp, data = source, None
        if case['io'] == 'full':
            if sys.platform == 'darwin':
                if bounded:
                    raise ValueError('Bounded release qualification uses Linux transports')
                env = cli_streams.native_full_environment(binary, env)
                out = stack.enter_context(open('/dev/null', 'wb'))
            else:
                out = stack.enter_context(open('/dev/full', 'wb'))
        if case['io'] == 'closed-pipe':
            read_fd, out = os.pipe()
            os.close(read_fd)
            stack.callback(os.close, out)
        if bounded:
            if case['io'] == 'directory' and not stat.S_ISDIR(os.fstat(inp).st_mode):
                raise ValueError('Bounded directory input differs')
            descriptor = out if isinstance(out, int) else out.fileno()
            cli_streams.admit_destination(descriptor,
                case['io'] if case['io'] in ('full', 'closed-pipe') else 'normal')
        cap = CAP // 2 if bounded else CAP
        def limits():
            resource.setrlimit(resource.RLIMIT_FSIZE, (cap, cap))
            if bounded:
                resource.setrlimit(resource.RLIMIT_CPU, (10, 10))
        started = time.monotonic()
        process = subprocess.Popen(argv, executable=str(binary), cwd=directory, env=env,
            stdin=inp, stdout=out, stderr=stderr_file, start_new_session=True,
            restore_signals=True,
            preexec_fn=limits)
        timed_out = False
        cleanup_confirmed = True
        def terminate():
            nonlocal cleanup_confirmed
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            try:
                process.communicate(timeout=0.5 if bounded else None)
            except subprocess.TimeoutExpired:
                cleanup_confirmed = False
                if process.stdin is not None:
                    process.stdin.close()
        try:
            if case.get('hold_stdin'):
                process.wait(timeout=max(0, 8 - (time.monotonic() - started)) if bounded else 8)
                process.stdin.close()
            else:
                process.communicate(data, timeout=max(0, 8 - (time.monotonic() - started))
                                    if bounded else 8)
        except subprocess.TimeoutExpired:
            timed_out = True
            terminate()
        except BaseException:
            terminate()
            raise
        stdout_file.seek(0)
        stderr_file.seek(0)
        stdout = stdout_file.read(CAP + 1)
        stderr = stderr_file.read(CAP + 1)
        return dict(argv=[os.fsdecode(a) for a in argv], executable=str(binary), env=env,
            process_group=process.pid,
            returncode=process.returncode, timed_out=timed_out,
            overflow=(len(stdout) >= cap or len(stderr) >= cap) if bounded else
                     (len(stdout) > CAP or len(stderr) > CAP),
            cleanup_confirmed=cleanup_confirmed,
            stdout_hex=stdout.hex(), stderr_hex=stderr.hex())


def normalize_sort_name(stderr_hex):
    """The system sort's diagnostics start with its program name: older GNU sort
    prints argv[0] (`/usr/bin/sort`, as Fastmash and GNU datamash run it), newer
    GNU sort and uutils print `sort`. Compare them as `sort`."""
    data = bytes.fromhex(stderr_hex)
    return re.sub(rb'(?m)^/usr/bin/sort: ', b'sort: ', data).hex()


def matches(case, observed):
    seen = {key: observed.get(key) for key in case['expected']}
    if isinstance(seen.get('stderr_hex'), str):
        seen['stderr_hex'] = normalize_sort_name(seen['stderr_hex'])
    return (not observed['timed_out'] and not observed.get('overflow', False)
            and seen == case['expected'])

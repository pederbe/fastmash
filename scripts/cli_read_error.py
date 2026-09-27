"""Bounded real Linux PTY input EIO after a visible completed group."""
import contextlib, errno, os, pty, resource, select, signal, subprocess, tempfile, time, tty

def invoke(case, binary, directory, cap):
    if signal.pthread_sigmask(signal.SIG_BLOCK, set()):
        raise RuntimeError('Blocked signal mask')
    data=bytes.fromhex(case['input_hex'])
    ack=bytes.fromhex(case['ack_hex'])
    if not 0<len(data)<=1024 or not ack:
        raise ValueError('Input/ack bounds')
    env={'PATH':'/usr/bin:/bin','LC_ALL':'C','TZ':'UTC'}
    argv=[case['name']]+case['args']
    with contextlib.ExitStack() as stack:
        descriptors=[]
        def close(fd):
            if fd in descriptors:
                descriptors.remove(fd)
                os.close(fd)
        def pair():
            master,slave=pty.openpty()
            descriptors.extend((master,slave))
            stack.callback(close,master)
            stack.callback(close,slave)
            tty.setraw(slave)
            return master,slave
        inp,producer=pair()
        receiver,out=pair()
        error=stack.enter_context(tempfile.TemporaryFile(dir=directory))
        deadline=time.monotonic()+8
        proc=subprocess.Popen(argv,executable=str(binary),cwd=directory,env=env,
            stdin=inp,stdout=out,stderr=error,start_new_session=True,restore_signals=True,
            preexec_fn=lambda: resource.setrlimit(resource.RLIMIT_FSIZE,(cap,cap)))
        close(inp)
        close(out)
        output=bytearray()
        acknowledged=False
        try:
            os.set_blocking(producer,False)
            if os.write(producer,data)!=len(data):
                raise RuntimeError('Short bounded input write')
            os.set_blocking(receiver,False)
            while True:
                if time.monotonic()>=deadline:
                    raise RuntimeError('Read-error deadline')
                if select.select([receiver],[],[],min(.05,max(0,deadline-time.monotonic())))[0]:
                    try: part=os.read(receiver,65536)
                    except OSError as exc:
                        if exc.errno!=errno.EIO: raise
                        part=b''
                    if not part: break
                    output.extend(part)
                    if len(output)>cap: raise RuntimeError('Capture overflow')
                    if not acknowledged and output.startswith(ack):
                        acknowledged=True
                        close(producer)
            proc.wait(timeout=max(.001,deadline-time.monotonic()))
            if not acknowledged: raise RuntimeError('No completed group acknowledgement')
        finally:
            if proc.poll() is None: os.killpg(proc.pid,signal.SIGKILL)
            proc.wait()
        stderr=os.pread(error.fileno(),cap+1,0)
        if len(stderr)>cap: raise RuntimeError('Error capture overflow')
        return dict(argv=argv,executable=str(binary),env=env,returncode=proc.returncode,
            timed_out=False,overflow=False,stdout_hex=output.hex(),stderr_hex=stderr.hex(),
            acknowledged=ack.hex(),input_error_mechanism='pty-master-after-slave-close')

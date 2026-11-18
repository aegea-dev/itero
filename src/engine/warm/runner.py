# itero's runner for a warm action: runs the action's file again for each
# request, in this process, so the libraries it imports stay loaded. It
# speaks to itero's worker on fd 3: a request is one JSON line in, its end
# one JSON line out. What the action prints goes to the real stdout and
# stderr, which the worker reads; a random mark on each ends a request's
# output. Runs on Python 3.8 and later.
import io, json, os, runpy, sys, threading, traceback

action = sys.argv[1]
base = dict(os.environ)
root = os.getcwd()
# as `python3 <file>` would have it: the file's folder first, for its imports
sys.path[0] = os.path.dirname(os.path.abspath(action))
requests = open(3, "r", encoding="utf-8", closefd=False)


def answer(text):
    data = text.encode("utf-8")
    while data:
        data = data[os.write(3, data):]


for line in requests:
    req = json.loads(line)
    os.environ.clear()
    os.environ.update(base)
    os.environ.update(req["env"])
    os.chdir(root)
    sys.argv = [action]
    sys.stdin = io.TextIOWrapper(io.BytesIO(req["stdin"].encode("utf-8")), encoding="utf-8")
    code = 0
    try:
        runpy.run_path(action, run_name="__main__")
    except SystemExit as e:
        if e.code is None:
            code = 0
        elif isinstance(e.code, int):
            code = e.code
        else:
            print(e.code, file=sys.stderr)
            code = 1
    except BaseException as e:
        # as python3 prints it: from the action's own frames on, without
        # this runner's and runpy's (none for a syntax error)
        tb = e.__traceback__
        while tb is not None and tb.tb_frame.f_code.co_filename != action:
            tb = tb.tb_next
        traceback.print_exception(type(e), e, tb)
        code = 1
    sys.stdout.flush()
    sys.stderr.flush()
    mark = ("\n" + req["mark"] + "\n").encode("utf-8")
    os.write(1, mark)
    os.write(2, mark)
    left = "a thread still runs" if threading.active_count() > 1 else None
    # the system keeps an exit code's low byte, as for a fresh process
    answer(json.dumps({"exit": code % 256, "left": left}) + "\n")

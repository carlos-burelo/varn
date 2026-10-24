import gzip
import json
import sys
import ctypes
from ctypes import wintypes
from collections import Counter

prof = sys.argv[1]
exe = sys.argv[2]
top = int(sys.argv[3]) if len(sys.argv) > 3 else 30
d = json.load(gzip.open(prof))
libs = d['libs']
vn_lib = next(i for i, l in enumerate(libs) if l['name'].lower() == 'vn.exe')

dbg = ctypes.WinDLL("dbghelp.dll", use_last_error=True)
ctypes.windll.kernel32.GetCurrentProcess.restype = wintypes.HANDLE
proc = ctypes.windll.kernel32.GetCurrentProcess()
dbg.SymInitialize.argtypes = [wintypes.HANDLE, wintypes.LPCSTR, wintypes.BOOL]
dbg.SymFromAddr.argtypes = [wintypes.HANDLE, ctypes.c_uint64, ctypes.POINTER(ctypes.c_uint64), ctypes.c_void_p]
dbg.SymSetOptions(0x2 | 0x10000000)
dbg.SymInitialize(proc, None, False)
BASE = 0x140000000
dbg.SymLoadModuleExW.restype = ctypes.c_uint64
dbg.SymLoadModuleExW.argtypes = [wintypes.HANDLE, wintypes.HANDLE, wintypes.LPCWSTR, wintypes.LPCWSTR,
                                 ctypes.c_uint64, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD]
b = dbg.SymLoadModuleExW(proc, None, exe.replace("/", "\\"), None, BASE, 0, None, 0)


class SYMBOL_INFO(ctypes.Structure):
    _fields_ = [('SizeOfStruct', wintypes.ULONG), ('TypeIndex', wintypes.ULONG),
                ('Reserved', ctypes.c_uint64 * 2), ('Index', wintypes.ULONG), ('Size', wintypes.ULONG),
                ('ModBase', ctypes.c_uint64), ('Flags', wintypes.ULONG), ('Value', ctypes.c_uint64),
                ('Address', ctypes.c_uint64), ('Register', wintypes.ULONG), ('Scope', wintypes.ULONG),
                ('Tag', wintypes.ULONG), ('NameLen', wintypes.ULONG), ('MaxNameLen', wintypes.ULONG),
                ('Name', ctypes.c_char * 1024)]


cache = {}


def sym(rva):
    if rva in cache:
        return cache[rva]
    si = SYMBOL_INFO()
    si.SizeOfStruct = 88
    si.MaxNameLen = 1024
    disp = ctypes.c_uint64(0)
    ok = dbg.SymFromAddr(proc, ctypes.c_uint64(BASE + rva), ctypes.byref(disp), ctypes.byref(si))
    name = si.Name.decode('utf8', 'replace') if ok else 'vn.exe+0x%x' % rva
    cache[rva] = name
    return name


threads = d['threads']
main = max(threads, key=lambda t: t['samples']['length'])
ft, fn, rt, st, sa = main['frameTable'], main['funcTable'], main['resourceTable'], main['stackTable'], main['samples']

frame_name = []
for i in range(ft['length']):
    f = ft['func'][i]
    res = fn['resource'][f]
    lib = rt['lib'][res] if res is not None and res >= 0 else None
    if lib == vn_lib:
        frame_name.append(sym(ft['address'][i]))
    else:
        frame_name.append('[%s]' % (libs[lib]['name'] if lib is not None else 'jit'))

self_t = Counter()
incl = Counter()
total = 0
for k in range(sa['length']):
    s = sa['stack'][k]
    if s is None:
        continue
    total += 1
    self_t[frame_name[st['frame'][s]]] += 1
    seen = set()
    cur = s
    while cur is not None:
        n = frame_name[st['frame'][cur]]
        if n not in seen:
            incl[n] += 1
            seen.add(n)
        cur = st['prefix'][cur]

print('samples', total)
print('--- self')
for n, c in self_t.most_common(top):
    print('%5.1f%% %s' % (100 * c / total, n[:130]))
print('--- inclusive')
for n, c in incl.most_common(top):
    print('%5.1f%% %s' % (100 * c / total, n[:130]))

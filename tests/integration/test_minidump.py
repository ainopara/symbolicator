import json

MINIDUMP_SUCCESS = {
    "status": "completed",
    "system_info": {
        "os_name": "Windows NT",
        "os_version": "10.0.14393",
        "os_build": "",
        "cpu_arch": "x86",
    },
    "crashed": True,
    "crash_reason": "EXCEPTION_ACCESS_VIOLATION_WRITE",
    "assertion": "",
    "stacktraces": [
        {
            "frames": [
                {
                    "status": "missing",
                    "original_index": 0,
                    "instruction_addr": "0x2a2a3d",
                },
                {
                    "status": "missing",
                    "original_index": 1,
                    "instruction_addr": "0x2a28d0",
                },
                {
                    "status": "symbolicated",
                    "original_index": 2,
                    "instruction_addr": "0x7584e9bf",
                    "package": "rpcrt4.dll",
                    "symbol": "?FreeWrapper@@YGXPAX@Z",
                    "sym_addr": "0x7584e960",
                    "function": "FreeWrapper(void *)",
                    "lineno": 0,
                },
                {
                    "status": "missing_symbol",
                    "original_index": 3,
                    "instruction_addr": "0x70850000",
                },
                {
                    "status": "symbolicated",
                    "original_index": 4,
                    "instruction_addr": "0x70b7ae3f",
                    "package": "dbgcore.dll",
                    "symbol": "?DetermineOutputProvider@@YGJPAVMiniDumpAllocationProvider@@PAXQAU_MINIDUMP_CALLBACK_INFORMATION@@PAPAVMiniDumpOutputProvider@@@Z",
                    "sym_addr": "0x70b7ad6b",
                    "function": "DetermineOutputProvider(class MiniDumpAllocationProvider *,void *,struct _MINIDUMP_CALLBACK_INFORMATION * const,class MiniDumpOutputProvider * *)",
                    "lineno": 0,
                },
                {
                    "status": "missing_symbol",
                    "original_index": 5,
                    "instruction_addr": "0x75810000",
                },
                {
                    "status": "symbolicated",
                    "original_index": 6,
                    "instruction_addr": "0x7584e9bf",
                    "package": "rpcrt4.dll",
                    "symbol": "?FreeWrapper@@YGXPAX@Z",
                    "sym_addr": "0x7584e960",
                    "function": "FreeWrapper(void *)",
                    "lineno": 0,
                },
                {
                    "status": "missing",
                    "original_index": 7,
                    "instruction_addr": "0x2a3435",
                },
                {
                    "status": "missing",
                    "original_index": 8,
                    "instruction_addr": "0x2a2d97",
                },
                {
                    "status": "symbolicated",
                    "original_index": 9,
                    "instruction_addr": "0x750662c3",
                    "package": "kernel32.dll",
                    "symbol": "@BaseThreadInitThunk@12",
                    "sym_addr": "0x750662a0",
                    "function": "@BaseThreadInitThunk@12",
                    "lineno": 0,
                },
                {
                    "status": "symbolicated",
                    "original_index": 10,
                    "instruction_addr": "0x771d0f78",
                    "package": "ntdll.dll",
                    "symbol": "__RtlUserThreadStart@8",
                    "sym_addr": "0x771d0f4a",
                    "function": "__RtlUserThreadStart@8",
                    "lineno": 0,
                },
                {
                    "status": "symbolicated",
                    "original_index": 11,
                    "instruction_addr": "0x771d0f43",
                    "package": "ntdll.dll",
                    "symbol": "_RtlUserThreadStart@8",
                    "sym_addr": "0x771d0f29",
                    "function": "_RtlUserThreadStart@8",
                    "lineno": 0,
                },
            ],
            "is_requesting": True,
        },
        {
            "frames": [
                {
                    "status": "symbolicated",
                    "original_index": 0,
                    "instruction_addr": "0x771e016c",
                    "package": "ntdll.dll",
                    "symbol": "ZwWaitForWorkViaWorkerFactory@20",
                    "sym_addr": "0x771e0160",
                    "function": "ZwWaitForWorkViaWorkerFactory@20",
                    "lineno": 0,
                },
                {
                    "status": "symbolicated",
                    "original_index": 1,
                    "instruction_addr": "0x771a6a10",
                    "package": "ntdll.dll",
                    "symbol": "TppWorkerThread@4",
                    "sym_addr": "0x771a6770",
                    "function": "TppWorkerThread@4",
                    "lineno": 0,
                },
                {
                    "status": "symbolicated",
                    "original_index": 2,
                    "instruction_addr": "0x750662c3",
                    "package": "kernel32.dll",
                    "symbol": "@BaseThreadInitThunk@12",
                    "sym_addr": "0x750662a0",
                    "function": "@BaseThreadInitThunk@12",
                    "lineno": 0,
                },
                {
                    "status": "symbolicated",
                    "original_index": 3,
                    "instruction_addr": "0x771d0f78",
                    "package": "ntdll.dll",
                    "symbol": "__RtlUserThreadStart@8",
                    "sym_addr": "0x771d0f4a",
                    "function": "__RtlUserThreadStart@8",
                    "lineno": 0,
                },
                {
                    "status": "symbolicated",
                    "original_index": 4,
                    "instruction_addr": "0x771d0f43",
                    "package": "ntdll.dll",
                    "symbol": "_RtlUserThreadStart@8",
                    "sym_addr": "0x771d0f29",
                    "function": "_RtlUserThreadStart@8",
                    "lineno": 0,
                },
            ],
            "is_requesting": False,
        },
        {
            "frames": [
                {
                    "status": "symbolicated",
                    "original_index": 0,
                    "instruction_addr": "0x771e016c",
                    "package": "ntdll.dll",
                    "symbol": "ZwWaitForWorkViaWorkerFactory@20",
                    "sym_addr": "0x771e0160",
                    "function": "ZwWaitForWorkViaWorkerFactory@20",
                    "lineno": 0,
                },
                {
                    "status": "symbolicated",
                    "original_index": 1,
                    "instruction_addr": "0x771a6a10",
                    "package": "ntdll.dll",
                    "symbol": "TppWorkerThread@4",
                    "sym_addr": "0x771a6770",
                    "function": "TppWorkerThread@4",
                    "lineno": 0,
                },
                {
                    "status": "symbolicated",
                    "original_index": 2,
                    "instruction_addr": "0x750662c3",
                    "package": "kernel32.dll",
                    "symbol": "@BaseThreadInitThunk@12",
                    "sym_addr": "0x750662a0",
                    "function": "@BaseThreadInitThunk@12",
                    "lineno": 0,
                },
                {
                    "status": "symbolicated",
                    "original_index": 3,
                    "instruction_addr": "0x771d0f78",
                    "package": "ntdll.dll",
                    "symbol": "__RtlUserThreadStart@8",
                    "sym_addr": "0x771d0f4a",
                    "function": "__RtlUserThreadStart@8",
                    "lineno": 0,
                },
                {
                    "status": "symbolicated",
                    "original_index": 4,
                    "instruction_addr": "0x771d0f43",
                    "package": "ntdll.dll",
                    "symbol": "_RtlUserThreadStart@8",
                    "sym_addr": "0x771d0f29",
                    "function": "_RtlUserThreadStart@8",
                    "lineno": 0,
                },
            ],
            "is_requesting": False,
        },
        {
            "frames": [
                {
                    "status": "symbolicated",
                    "original_index": 0,
                    "instruction_addr": "0x771df3dc",
                    "package": "ntdll.dll",
                    "symbol": "ZwGetContextThread@8",
                    "sym_addr": "0x771df3d0",
                    "function": "ZwGetContextThread@8",
                    "lineno": 0,
                },
                {
                    "status": "symbolicated",
                    "original_index": 1,
                    "instruction_addr": "0x76e75dbf",
                    "package": "KERNELBASE.dll",
                    "symbol": "NlsIsUserDefaultLocale@4",
                    "sym_addr": "0x76e75d90",
                    "function": "NlsIsUserDefaultLocale@4",
                    "lineno": 0,
                },
            ],
            "is_requesting": False,
        },
    ],
    "modules": [
        {
            "debug_status": "missing",
            "unwind_status": "missing",
            "arch": "unknown",
            "type": "pe",
            "code_id": "5AB380779000",
            "code_file": "crash.exe",
            "debug_id": "3249D99D0C4049318610F4E4FB0B69361",
            "debug_file": "crash.pdb",
            "image_addr": "0x2a0000",
            "image_size": 36864,
        },
        {
            "arch": "x86",
            "code_file": "dbghelp.dll",
            "code_id": "57898E12145000",
            "debug_file": "dbghelp.pdb",
            "debug_id": "9C2A902B6FDF40AD8308588A41D572A01",
            "image_addr": "0x70850000",
            "image_size": 1_331_200,
            "debug_status": "found",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "arch": "unknown",
            "code_file": "msvcp140.dll",
            "code_id": "589ABC846c000",
            "debug_file": "msvcp140.i386.pdb",
            "debug_id": "BF5257F78C2643DD9BB7901625E1136A1",
            "image_addr": "0x709a0000",
            "image_size": 442_368,
            "debug_status": "unused",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "arch": "unknown",
            "code_file": "apphelp.dll",
            "code_id": "57898EEB92000",
            "debug_file": "apphelp.pdb",
            "debug_id": "8DAF7773372F460AAF38944E193F7E331",
            "image_addr": "0x70a10000",
            "image_size": 598_016,
            "debug_status": "unused",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "debug_status": "found",
            "unwind_status": "found",
            "arch": "x86",
            "type": "pe",
            "code_id": "57898DAB25000",
            "code_file": "dbgcore.dll",
            "debug_id": "AEC7EF2FDF4B4642A4714C3E5FE8760A1",
            "debug_file": "dbgcore.pdb",
            "image_addr": "0x70b70000",
            "image_size": 151_552,
        },
        {
            "arch": "unknown",
            "code_file": "VCRUNTIME140.dll",
            "code_id": "589ABC7714000",
            "debug_file": "vcruntime140.i386.pdb",
            "debug_id": "0ED80A50ECDA472B86A4EB6C833F8E1B1",
            "image_addr": "0x70c60000",
            "image_size": 81920,
            "debug_status": "unused",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "arch": "unknown",
            "code_file": "CRYPTBASE.dll",
            "code_id": "57899141a000",
            "debug_file": "cryptbase.pdb",
            "debug_id": "147C51FB7CA1408F85B5285F2AD6F9C51",
            "image_addr": "0x73ba0000",
            "image_size": 40960,
            "debug_status": "unused",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "arch": "unknown",
            "code_file": "sspicli.dll",
            "code_id": "59BF30E31f000",
            "debug_file": "wsspicli.pdb",
            "debug_id": "51E432B104504B198ED16D4335F9F5431",
            "image_addr": "0x73bb0000",
            "image_size": 126_976,
            "debug_status": "unused",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "arch": "unknown",
            "code_file": "advapi32.dll",
            "code_id": "5A49BB7677000",
            "debug_file": "advapi32.pdb",
            "debug_id": "0C799483B549417D84334331852031FE1",
            "image_addr": "0x73c70000",
            "image_size": 487_424,
            "debug_status": "unused",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "arch": "unknown",
            "code_file": "msvcrt.dll",
            "code_id": "57899155be000",
            "debug_file": "msvcrt.pdb",
            "debug_id": "6F6409B3D52043C79B2F62E00BFE761C1",
            "image_addr": "0x73cf0000",
            "image_size": 778_240,
            "debug_status": "unused",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "arch": "unknown",
            "code_file": "sechost.dll",
            "code_id": "598942C741000",
            "debug_file": "sechost.pdb",
            "debug_id": "6F6A05DD0A80478BA4199B88703BF75B1",
            "image_addr": "0x74450000",
            "image_size": 266_240,
            "debug_status": "unused",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "debug_status": "found",
            "unwind_status": "found",
            "arch": "x86",
            "type": "pe",
            "code_id": "590285E9e0000",
            "code_file": "kernel32.dll",
            "debug_id": "D347455996F747D6BF43C176B2171E681",
            "debug_file": "wkernel32.pdb",
            "image_addr": "0x75050000",
            "image_size": 917_504,
            "type": "pe",
        },
        {
            "arch": "unknown",
            "code_file": "bcryptPrimitives.dll",
            "code_id": "59B0DF8F5a000",
            "debug_file": "bcryptprimitives.pdb",
            "debug_id": "287B19C392094A2BBB8FBCC37F411B111",
            "image_addr": "0x75130000",
            "image_size": 368_640,
            "debug_status": "unused",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "debug_status": "found",
            "unwind_status": "found",
            "arch": "x86",
            "type": "pe",
            "code_id": "5A49BB75c1000",
            "code_file": "rpcrt4.dll",
            "debug_id": "AE131C6727A74FA19916B5A4AEF411901",
            "debug_file": "wrpcrt4.pdb",
            "image_addr": "0x75810000",
            "image_size": 790_528,
        },
        {
            "arch": "unknown",
            "code_file": "ucrtbase.dll",
            "code_id": "59BF2B5Ae0000",
            "debug_file": "ucrtbase.pdb",
            "debug_id": "6BEDCBCE0A3A40E9804081C2C8C6CC2F1",
            "image_addr": "0x758f0000",
            "image_size": 917_504,
            "debug_status": "unused",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "arch": "x86",
            "code_file": "KERNELBASE.dll",
            "code_id": "59BF2BCF1a1000",
            "debug_file": "wkernelbase.pdb",
            "debug_id": "8462294AC645402DAC82A4E95F61DDF91",
            "image_addr": "0x76db0000",
            "image_size": 1_708_032,
            "debug_status": "found",
            "unwind_status": "unused",
            "type": "pe",
        },
        {
            "debug_status": "found",
            "unwind_status": "found",
            "arch": "x86",
            "type": "pe",
            "code_id": "59B0D8F3183000",
            "code_file": "ntdll.dll",
            "debug_id": "971F98E5CE6041FFB2D7235BBEB345781",
            "debug_file": "wntdll.pdb",
            "image_addr": "0x77170000",
            "image_size": 1_585_152,
        },
    ],
}


def test_basic(symbolicator, hitcounter):
    service = symbolicator()
    service.wait_healthcheck()

    with open("tests/fixtures/windows.dmp", "rb") as f:
        response = service.post(
            "/minidump",
            files={"upload_file_minidump": f},
            data={
                "sources": json.dumps(
                    [
                        {
                            "type": "http",
                            "id": "microsoft",
                            "layout": {"type": "symstore"},
                            "filetypes": ["pdb", "pe"],
                            "url": f"{hitcounter.url}/msdl/",
                            "is_public": True,
                        }
                    ]
                )
            },
        )
        response.raise_for_status()

    assert response.json() == MINIDUMP_SUCCESS

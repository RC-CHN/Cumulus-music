"""Inspect a caller-supplied hash-pinned PE; never import or execute it natively."""
import hashlib
import struct
from pathlib import Path

import capstone
import pefile
from capstone.x86_const import X86_OP_MEM, X86_REG_RAX

DLL_SHA256 = 'c59105e1072d7d7dbd2d2844ac9c170de7e7b41eb463dc56a11c9b37bafac6be'
BASE = 0x180000000


class NativeImage:
    def __init__(self, path):
        path = Path(path)
        if not path.is_file() or path.stat().st_size > 8 * 1024 * 1024:
            raise ValueError('expected bounded local DLL file')
        self.raw = path.read_bytes()
        if hashlib.sha256(self.raw).hexdigest() != DLL_SHA256:
            raise ValueError('original DLL SHA256 mismatch')
        pe = pefile.PE(data=self.raw, fast_load=True)
        self.base = pe.OPTIONAL_HEADER.ImageBase
        if self.base != BASE or pe.FILE_HEADER.Machine != 0x8664:
            raise ValueError('expected original x64 PE layout')
        self.data = pe.get_memory_mapped_image()
        if len(self.data) != 0x3A2400:
            raise ValueError('unexpected mapped image extent')
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
        self.md.detail = True

    def resolve(self, address):
        for _ in range(8):
            if not 0x1000 <= address < 0x1E3000:
                raise ValueError('method outside original executable range')
            if self.data[address] != 0xE9:
                return address
            address += 5 + struct.unpack_from('<i', self.data, address + 1)[0]
        raise ValueError('jump cycle')

    def slot(self, vtable, offset):
        if not 0x1E3000 <= vtable + offset <= len(self.data) - 8:
            raise ValueError('vtable outside original image data')
        address = struct.unpack_from('<Q', self.data, vtable + offset)[0]
        return self.resolve(address - self.base)

    def graph_return_sites(self):
        # These are passive observation addresses following the actual 22
        # virtual child Process calls in original 1AA50, not replacement code.
        sites = []
        for ins in self.md.disasm(self.data[0x1AA50:0x1AD6B], self.base + 0x1AA50):
            if ins.mnemonic != 'call' or len(ins.operands) != 1:
                continue
            op = ins.operands[0]
            if (op.type == X86_OP_MEM and op.mem.base == X86_REG_RAX
                    and op.mem.index == 0 and op.mem.disp == 0x10):
                sites.append(ins.address + ins.size)
        if len(sites) != 22 or len(set(sites)) != 22:
            raise ValueError('original graph child callsites changed')
        return sites

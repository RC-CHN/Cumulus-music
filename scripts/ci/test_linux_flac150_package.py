from pathlib import Path
import tempfile
import unittest

import linux_flac150_package as package


DYNAMIC = '''
 0x0000000000000001 (NEEDED)             Shared library: [libm.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x000000000000000e (SONAME)             Library soname: [libcumulus_flac150.so.14]
 0x000000000000001e (FLAGS)              SYMBOLIC
'''
RELOC = '0000000000041000 0000000000000008 R_X86_64_RELATIVE 41000'


def exports():
    return '\n'.join([
        ' 1: 0000000000000100 8 OBJECT GLOBAL DEFAULT 20 FLAC__VERSION_STRING',
        *(f' 2: 0000000000000200 16 FUNC GLOBAL DEFAULT 8 {name}' for name in package.DECODER_SYMBOLS),
    ])


def binding(source, target, name):
    return f"     4021: binding file {source} [0] to {target} [0]: normal symbol `{name}'\n"


class FlacPackageContracts(unittest.TestCase):
    def test_private_soname_system_closure_and_symbolic_flag_are_mandatory(self):
        self.assertEqual(package.dynamic_contract(DYNAMIC), ['libc.so.6', 'libm.so.6'])
        bad = [
            DYNAMIC.replace(package.SONAME, 'libFLAC.so.14'),
            DYNAMIC.replace('libm.so.6', 'libogg.so.0'),
            DYNAMIC + ' 0x0000000000000001 (NEEDED) Shared library: [libc.so.6]\n',
            DYNAMIC.replace('SYMBOLIC', 'NOW'),
            DYNAMIC + ' 0x000000000000001d (RUNPATH) Library runpath: [$ORIGIN]\n',
            DYNAMIC + ' 0x000000000000000f (RPATH) Library rpath: [/usr/local/lib]\n',
            DYNAMIC + ' 0x000000006ffffefb (DEPAUDIT) Dependency audit library: [bad.so]\n',
            DYNAMIC.replace('SYMBOLIC', 'SYMBOLIC TEXTREL'),
        ]
        for data in bad:
            with self.subTest(data=data), self.assertRaises(ValueError):
                package.dynamic_contract(data)

    def test_reject_preemptible_data_or_function_and_missing_abi(self):
        symbols = exports()
        package.export_contract(symbols, RELOC)
        for kind in ['R_X86_64_GLOB_DAT', 'R_X86_64_JUMP_SLOT']:
            with self.subTest(kind=kind), self.assertRaisesRegex(ValueError, 'interposable'):
                package.export_contract(symbols, f'{RELOC}\n000 000 {kind} 000 FLAC__VERSION_STRING + 0')
        with self.assertRaisesRegex(ValueError, 'interposable'):
            package.export_contract(symbols + '\n 3: 0 0 FUNC GLOBAL DEFAULT UND FLAC__foreign', RELOC)
        with self.assertRaisesRegex(ValueError, 'decoder ABI'):
            package.export_contract(symbols.replace(package.DECODER_SYMBOLS[-1], 'unrelated'), RELOC)
        with self.assertRaisesRegex(ValueError, 'relocations'):
            package.export_contract(symbols, '')

    def test_binding_trace_distinguishes_live_entry_owner_from_internal_mixing(self):
        app = '/private package/libcumulus_flac150.so.14'
        ambient = '/usr/lib/libFLAC.so.14.0.0'
        new = package.DECODER_SYMBOLS[0]
        good = binding('_ctypes.so', ambient, new) + binding('_ctypes.so', app, new)
        self.assertEqual(package.binding_contract(good, app, ambient)['foreignFlacSymbolBindings'], 0)
        for symbol in ['FLAC__VERSION_STRING', package.DECODER_SYMBOLS[-1]]:
            with self.subTest(symbol=symbol), self.assertRaisesRegex(ValueError, 'another file'):
                package.binding_contract(good + binding(app, ambient, symbol), app, ambient)
        with self.assertRaisesRegex(ValueError, 'no package decoder'):
            package.binding_contract('', app, ambient)
        with self.assertRaisesRegex(ValueError, 'no ambient decoder'):
            package.binding_contract(binding('_ctypes.so', app, new), app, ambient)

    def test_reject_invalid_elf_or_link_before_invoking_readelf(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            invalid = root / package.SONAME
            invalid.write_bytes(b'not an ELF')
            with self.assertRaisesRegex(ValueError, 'x86_64 shared ELF'):
                package.audit_elf(invalid)
            linked = root / 'linked.so'
            linked.symlink_to(invalid)
            with self.assertRaisesRegex(ValueError, 'bounded regular file'):
                package.audit_elf(linked)

    def test_local_archive_is_only_a_supply_channel_for_exact_locked_bytes(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            source, out = root / 'arbitrary.tar.xz', root / 'staged.tar.xz'
            source.write_bytes(b'not locked upstream')
            with self.assertRaisesRegex(ValueError, 'SHA256 mismatch'):
                package.prepare_archive(out, source)
            self.assertFalse(out.exists())


if __name__ == '__main__':
    unittest.main()

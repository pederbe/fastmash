"""Preserve frozen results while adapting native fault observations."""
import copy
import unittest
from unittest import mock

import check_regressions as runner
import regression_cases


class NativeExpectations(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.cases = runner.load_fixture(runner.FIXTURE)
        cls.by_id = {case['id']: case for case in cls.cases}

    def test_every_native_disposition_preserves_completed_stdout_and_status(self):
        named = runner.NATIVE_SORT_DIAGNOSTICS | runner.NATIVE_BUFFER_OBSERVATIONS
        self.assertTrue(named <= self.by_id.keys())
        for case in self.cases:
            original = copy.deepcopy(case)
            for size in (0, 512, 4096, 8192, 16384):
                expected = runner.expected(case, platform='darwin',
                    destination={'isatty': False, 'block_size': size})
                frozen = runner.expected(case, platform='linux')
                self.assertEqual(expected.get('stdout_hex'), frozen.get('stdout_hex'), case['id'])
                self.assertEqual(expected['returncode'], frozen['returncode'], case['id'])
            self.assertEqual(case, original)

    def test_linux_expectations_and_unknown_native_diagnostics_are_exact(self):
        for case in self.cases:
            frozen = runner.expected(case, platform='linux')
            if case['id'] not in runner.NATIVE_SORT_DIAGNOSTICS:
                self.assertEqual(runner.expected(case, platform='darwin').get('stderr_hex'),
                                 frozen.get('stderr_hex'), case['id'])
            if 'stdout_file' not in case['expected']:
                self.assertEqual(frozen, case['expected'])
        unknown = copy.deepcopy(self.by_id['sorting/header-empty'])
        unknown['id'] = 'a-new-sort-diagnostic'
        self.assertEqual(runner.expected(unknown, platform='darwin'), unknown['expected'])

    def test_native_missing_header_is_an_exact_diagnostic(self):
        for name in runner.NATIVE_SORT_DIAGNOSTICS:
            expected = runner.expected(self.by_id[name], platform='darwin')
            self.assertEqual(bytes.fromhex(expected['stderr_hex']),
                             b'fastmash: missing input header for named grouping key\n')
            self.assertEqual(expected['returncode'], 1)

    def test_held_group_prefix_uses_fixture_bytes_and_native_metadata(self):
        case = self.by_id['grouping-reference-r2:held-flush-pipe']
        for block_size, length in ((4096, 4096), (8192, 0), (512, 6656), (0, 0)):
            expected = runner.expected(case, platform='darwin',
                destination={'isatty': False, 'block_size': block_size})
            self.assertEqual(expected['held']['bytes'], length)
            self.assertTrue(expected['held']['alive'])
            self.assertEqual(expected['held']['stdout_hex'],
                             expected['stdout_hex'][:length * 2])
        for facts in ({'isatty': True, 'block_size': 4096},
                      {'isatty': False, 'block_size': -1},
                      {'isatty': False, 'block_size': '4096'}):
            with self.assertRaises(ValueError):
                runner.expected(case, platform='darwin', destination=facts)

    def test_wrong_native_stdout_or_status_cannot_match(self):
        case = self.by_id['sorting/header-empty']
        observed = dict(runner.expected(case, platform='darwin'),
                        timed_out=False, overflow=False)
        with mock.patch.object(runner.sys, 'platform', 'darwin'):
            self.assertTrue(runner.matches(case, observed))
            for key, value in (('stdout_hex', b'incorrect\n'.hex()), ('returncode', 0),
                               ('stderr_hex', b'fastmash: another error\n'.hex())):
                self.assertFalse(runner.matches(case, dict(observed, **{key: value})))

    def test_timeout_and_overflow_never_match(self):
        case = self.by_id['version']
        observed = dict(runner.expected(case, platform='linux'),
                        timed_out=False, overflow=False)
        self.assertTrue(regression_cases.matches(case, observed))
        self.assertFalse(regression_cases.matches(case, dict(observed, timed_out=True)))
        self.assertFalse(regression_cases.matches(case, dict(observed, overflow=True)))

    def test_regular_file_inventory_keeps_all_ordinary_input(self):
        applicable = [case for case in self.cases if runner.file_input(case)]
        excluded = [case for case in self.cases if not runner.file_input(case)]
        self.assertEqual(len(applicable) + len(excluded), 3137)
        self.assertTrue(all(case['io'] == 'normal' for case in applicable))
        self.assertTrue(all(case['io'] != 'normal' or case.get('observe_held')
                            or case.get('hold_stdin') for case in excluded))


if __name__ == '__main__':
    unittest.main()

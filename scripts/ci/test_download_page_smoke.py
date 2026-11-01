"""A DOM trace alone cannot establish complete virtualized MV coverage."""
import copy
import unittest
from download_page_smoke import verify_mv_coverage


def proof():
    native = [f'mv-{i}' for i in range(129)]
    return {'nativeIds': native, 'viewportSamples': [
        {'y': start, 'scrollTop': start, 'cards': [
            {'index': i, 'id': native[i]} for i in range(start, min(129, start + 50))
        ]} for start in [0, 40, 80, 120]
    ]}


class MvCoverage(unittest.TestCase):
    def test_complete_overlapping_viewports_are_valid(self):
        verify_mv_coverage(proof())

    def test_missing_first_34_items_is_not_full_coverage(self):
        value = proof()
        for view in value['viewportSamples']:
            view['cards'] = [c for c in view['cards'] if c['index'] >= 34]
        with self.assertRaises(AssertionError):
            verify_mv_coverage(value)

    def test_recycled_card_or_duplicate_native_id_is_rejected(self):
        value = proof()
        bad = copy.deepcopy(value)
        bad['viewportSamples'][0]['cards'][0]['id'] = value['nativeIds'][34]
        with self.assertRaises(AssertionError):
            verify_mv_coverage(bad)
        value['nativeIds'][0] = value['nativeIds'][1]
        with self.assertRaises(AssertionError):
            verify_mv_coverage(value)

    def test_unsettled_or_out_of_range_viewports_are_rejected(self):
        value = proof()
        value['viewportSamples'][0]['scrollTop'] = 200
        with self.assertRaises(AssertionError):
            verify_mv_coverage(value)
        value = proof()
        value['viewportSamples'][0]['cards'][0]['index'] = 129
        with self.assertRaises(AssertionError):
            verify_mv_coverage(value)

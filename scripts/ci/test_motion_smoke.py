"""A frozen fractional-DPI focus sample must address the containing pixel."""
import unittest
from motion_smoke import paint_checks


class FractionalPaintCoordinates(unittest.TestCase):
    def test_125_percent_outline_does_not_sample_its_adjacent_background(self):
        # Observed synthetic quality-menu bounds, no account or image data.
        # The red outline ends at pixel 840; pixel 841 is the hovered surface.
        class Picture:
            def __init__(self, outlined=True): self.outlined = outlined
            def get_width(self): return 950
            def get_height(self): return 675
            def get_rowstride(self): return 950 * 3
            def get_n_channels(self): return 3
            def get_pixels(self):
                data = bytearray(950 * 675 * 3)
                for x, color in [(840, (204, 41, 72) if self.outlined else (238, 234, 242)), (841, (238, 234, 242))]:
                    offset = (512 * 950 + x) * 3
                    data[offset:offset + 3] = bytes(color)
                return data
        payload = {'ratio': 1.25, 'paintSamples': [
            {'x': 672.4000244140625, 'y': 410.1874942779541, 'color': 'rgb(204, 41, 72)'},
        ]}
        checks = paint_checks(Picture(), payload)
        self.assertEqual(checks[0]['point'], [840, 512])
        self.assertTrue(checks[0]['ok'])
        # The same coordinate must still fail if the expected outline really
        # is absent; no color tolerance or ignored failure was introduced.
        self.assertFalse(paint_checks(Picture(outlined=False), payload)[0]['ok'])


if __name__ == '__main__':
    unittest.main()

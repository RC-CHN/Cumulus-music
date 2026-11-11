"""The Windows EOF proof restores only the cursor position it still owns."""
import unittest

from natural_eof_smoke import POINTER_PARK, parked_windows_cursor


class Cursor:
    def __init__(self, point):
        self.point = point

    def position(self):
        return self.point

    def move(self, point):
        self.point = point


class ParkedCursorTests(unittest.TestCase):
    def test_restores_cursor_even_when_the_probe_fails(self):
        cursor = Cursor((-20, 30))
        with self.assertRaisesRegex(RuntimeError, 'synthetic probe failure'):
            with parked_windows_cursor(cursor) as parked:
                self.assertEqual(parked, POINTER_PARK)
                self.assertEqual(cursor.position(), parked)
                raise RuntimeError('synthetic probe failure')
        self.assertEqual(cursor.position(), (-20, 30))

    def test_does_not_restore_over_later_cursor_movement(self):
        cursor = Cursor((20, 30))
        with parked_windows_cursor(cursor):
            cursor.move((50, 60))
        self.assertEqual(cursor.position(), (50, 60))


if __name__ == '__main__':
    unittest.main()

import unittest
from protocol import decode, encode

class ProtocolTests(unittest.TestCase):
    def test_round_trip(self):
        value = {"type":"start_job","job_id":"job_1","profile_id":"modal_01","input_path":"a.png","prompt":"move"}
        self.assertEqual(decode(encode(value)), value)
    def test_required_fields(self):
        with self.assertRaises(ValueError): decode('{"type":"start_job","job_id":"job_1"}')

if __name__ == '__main__': unittest.main()

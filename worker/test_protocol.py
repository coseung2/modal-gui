import unittest

try:
    from .protocol import decode, encode
except ImportError:
    from protocol import decode, encode

class ProtocolTests(unittest.TestCase):
    def test_round_trip(self):
        value = {"type":"start_job","job_id":"job_1","profile_id":"modal_01","input_path":"a.png","prompt":"move"}
        self.assertEqual(decode(encode(value)), value)
    def test_required_fields(self):
        with self.assertRaises(ValueError): decode('{"type":"start_job","job_id":"job_1","profile_id":"modal_01"}')

    def test_text_to_video_job_does_not_need_an_image(self):
        value = {"type":"start_job","job_id":"job_2","profile_id":"modal_01","kind":"t2v","prompt":"move"}
        self.assertEqual(decode(encode(value)), value)

    def test_music_job_requires_style_and_lyrics(self):
        value = {"type":"start_music","job_id":"job_3","profile_id":"modal_01","style":"electronic rock","lyrics":"[Chorus] Drop in"}
        self.assertEqual(decode(encode(value)), value)

if __name__ == '__main__': unittest.main()

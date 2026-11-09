stereo-44100.mp3 is a synthetic 660 Hz sine, generated locally with
FFmpeg 8.0.1-3ubuntu2 and libmp3lame. There are no downloaded recordings.

ffmpeg -v error -f lavfi -i 'sine=frequency=660:sample_rate=44100:duration=0.5' -ac 2 -c:a libmp3lame -b:a 128k -id3v2_version 0 -write_xing 0 -write_id3v1 0 -map_metadata -1 -f mp3 stereo-44100.mp3

8777 bytes
SHA-256: 9cd5e6cd5f81696315476e61f27e818027fcce1de3ebf2e6834860f30e49d698

stereo-44100.mp3 is a synthetic 660 Hz sine, generated locally with
FFmpeg 8.0.1-3ubuntu2 and libmp3lame. There are no downloaded recordings.

ffmpeg -v error -f lavfi -i 'sine=frequency=660:sample_rate=44100:duration=0.5' -ac 2 -c:a libmp3lame -b:a 128k -id3v2_version 0 -write_xing 0 -write_id3v1 0 -map_metadata -1 -f mp3 stereo-44100.mp3

8777 bytes
SHA-256: 9cd5e6cd5f81696315476e61f27e818027fcce1de3ebf2e6834860f30e49d698

search-playback-8s.flac is a synthetic 439 Hz sine for the search playback
clock proof, generated with FFmpeg 8.0.1-3ubuntu2. It is not used for download
byte verification, which continues to use the separate tag_audio seed.

ffmpeg -v error -f lavfi -i 'sine=frequency=439:sample_rate=8000:duration=8' -map_metadata -1 -c:a flac -compression_level 8 -bitexact search-playback-8s.flac

38531 bytes
SHA-256: 8bd8b67fc43432ae445548a57d59842ec82843a28343fc380c7c11197d949535

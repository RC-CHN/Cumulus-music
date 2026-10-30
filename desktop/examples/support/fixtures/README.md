# Offline video UI fixture

`offline-mv.mp4` is a synthetic six-second moving test pattern, 160×90 at 12 fps.
It lets the desktop probe observe actual decoded pixels independently of metadata
reads. The resolution stored in the synthetic download task tests presentation
of service metadata; it does not describe the encoded dimensions.
The tests read this checked-in file and do not require FFmpeg to generate it.

    ffmpeg -nostdin -f lavfi -i testsrc2=size=160x90:rate=12 -t 6 -an -c:v mpeg4 -q:v 8 -threads 1 -flags:v +bitexact -fflags +bitexact -map_metadata -1 -movflags +faststart offline-mv.mp4

SHA-256: `877de212e6f9aca202ab647f7af0e3840cfbbfb9dad2aa3713916bfad61c92ac`.

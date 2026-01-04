#!/usr/bin/env -S bash -xe

OUTPUT_FILE="$1"
IMAGE_PATH="$2"
IMAGE_MIME_TYPE="image/jpeg"

# Export existing comments to file.
COMMENTS_PATH="$(command mktemp -t "tmp.XXXXXXXXXX")"
command vorbiscomment --list --raw "${OUTPUT_FILE}" > "${COMMENTS_PATH}"

# Remove existing images.
command sed -i -e '/^metadata_block_picture/d' "${COMMENTS_PATH}"

# Insert cover image from file.

# metadata_block_picture format.
# See: https://xiph.org/flac/format.html#metadata_block_picture

IMAGE_WITH_HEADER="$(command mktemp -t "tmp.XXXXXXXXXX")"
DESCRIPTION=""

# Reset cache file.
echo -n "" > "${IMAGE_WITH_HEADER}"

# Picture type <32>.
command printf "0: %.8x" 3 | command xxd -r -g0 \
        >> "${IMAGE_WITH_HEADER}"
# Mime type length <32>.
command printf "0: %.8x" $(echo -n "${IMAGE_MIME_TYPE}" | command wc -c) \
          | command xxd -r -g0 \
        >> "${IMAGE_WITH_HEADER}"
# Mime type (n * 8)
echo -n "${IMAGE_MIME_TYPE}" >> "${IMAGE_WITH_HEADER}"
# Description length <32>.
command printf "0: %.8x" $(echo -n "${DESCRIPTION}" | command wc -c) \
          | command xxd -r -g0 \
        >> "${IMAGE_WITH_HEADER}"
# Description (n * 8)
echo -n "${DESCRIPTION}" >> "${IMAGE_WITH_HEADER}"
# Picture with <32>.
command printf "0: %.8x" 0 | command xxd -r -g0 \
        >> "${IMAGE_WITH_HEADER}"
# Picture height <32>.
command printf "0: %.8x" 0 | command xxd -r -g0 \
        >> "${IMAGE_WITH_HEADER}"
# Picture color depth <32>.
command printf "0: %.8x" 0 | command xxd -r -g0 \
        >> "${IMAGE_WITH_HEADER}"
# Picture color count <32>.
command printf "0: %.8x" 0 | command xxd -r -g0 \
        >> "${IMAGE_WITH_HEADER}"
# Image file size <32>.
command printf "0: %.8x" $(command wc -c "${IMAGE_PATH}" \
          | command cut --delimiter=' ' --fields=1) \
          | command xxd -r -g0 \
        >> "${IMAGE_WITH_HEADER}"
# Image file.
command cat "${IMAGE_PATH}" >> "${IMAGE_WITH_HEADER}"

echo "metadata_block_picture=$(command base64 --wrap=0 < "${IMAGE_WITH_HEADER}")" >> "${COMMENTS_PATH}"

# Update vorbis file comments.
command vorbiscomment --write --raw --commentfile "${COMMENTS_PATH}" "${OUTPUT_FILE}"

# Delete cache file.
command rm "${IMAGE_WITH_HEADER}"
# Delete comments file.
command rm "${COMMENTS_PATH}"

exit 0

# use with `oggart /path/to/music_file.ogg /path/to/image_file -e`

FILE1="$(basename $1)"
EXT1=${FILE1##*.}
EXTTYPE1=$(echo $EXT1 | tr '[:upper:]' '[:lower:]')

FILE2="$(basename $2)"
EXT2=${FILE2##*.}
EXTTYPE2=$(echo $EXT2 | tr '[:upper:]' '[:lower:]')

OGG=""
if [[ "${EXTTYPE1}" == "ogg" ]]; then
  OGG="$1"
elif [[ "${EXTTYPE2}" == "ogg" ]]; then
  OGG="$2"
fi
if [[ -z "$OGG" ]]; then
  echo "no ogg file selected"
  exit 0
fi

PIC=""
array=(jpeg jpg png)
for item in ${array[*]}
do
if [ "$item" = "$EXTTYPE1" ]; then
PIC="$1"
elif [ "$item" = "$EXTTYPE2" ]; then
PIC="$2"
fi
done
if [ "$PIC" = "" ]; then
echo no jpg or png file selected
exit 0
fi

if [ "$3" = -e ]; then
EASYTAG=Y
else
EASYTAG=N
fi

DESC=$(basename "$PIC")
APIC=$(base64 --wrap=0 "$PIC")
if [ "$(which exiv2)" != "" ]; then
  MIME=$(exiv2 "$PIC" | grep 'MIME type ' | sed 's/: /|/' | cut -f 2 -d '|' | tail -n 1)
fi
if [ "$MIME" = "" ]; then
MIME="image/jpeg"
fi

vorbiscomment -l "$OGG" | grep -v '^COVERART=' | grep -v '^COVERARTDESCRIPTION=' | grep -v '^COVERARTMIME=' | grep -v 'METADATA_BLOCK_PICTURE=' > "$OGG".tags

if [ "$EASYTAG" = N ]; then
echo METADATA_BLOCK_PICTURE="$APIC" > "$OGG".tags2
else
echo COVERART="$APIC" > "$OGG".tags2
fi
vorbiscomment -w -R -c "$OGG".tags2 "$OGG"
# vorbiscomment -a -R -t COVERARTDESCRIPTION="$DESC" "$OGG"
# vorbiscomment -a -R -t COVERARTMIME="$MIME" "$OGG"
vorbiscomment -a -R -c "$OGG".tags "$OGG"

rm -f "$OGG".tags
rm -f "$OGG".tags2

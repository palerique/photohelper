/* photohelper_libraw_shim.c — minimal C ABI over LibRaw's struct types.
 *
 * LibRaw's C API returns pointers to large structs (libraw_iparams_t,
 * libraw_imgother_t, ...). Reading fields from those structs in Rust
 * would require #[repr(C)] mirrors that drift with every LibRaw patch
 * release. This shim exposes only the typed/numeric values photohelper
 * actually needs, so the Rust FFI never has to know the struct layouts.
 *
 * Every function below is single-line and side-effect-free. Reviewers
 * can audit the whole surface in one sitting.
 */
#include <libraw/libraw.h>
#include <stdint.h>

/* === EXIF accessors (RawExif inputs) ============================= */

/* Camera make string, NUL-terminated, owned by LibRaw. The Rust
 * caller must copy it before calling libraw_close(). */
const char *ph_libraw_make(libraw_data_t *lr) {
    return libraw_get_iparams(lr)->make;
}

/* Camera model string, NUL-terminated, owned by LibRaw. */
const char *ph_libraw_model(libraw_data_t *lr) {
    return libraw_get_iparams(lr)->model;
}

/* Post-rotation image orientation as LibRaw's `flip` value.
 * Maps to EXIF orientation via:
 *   flip 0 -> EXIF Normal (1)
 *   flip 3 -> EXIF Rotate180 (3)
 *   flip 5 -> EXIF Rotate90Cw (6)  -- LibRaw conflates these two
 *   flip 6 -> EXIF Rotate270Cw (8) -- LibRaw conflates these two
 * Conversion in Rust to disambiguate against ExifOrientation. */
int32_t ph_libraw_flip(libraw_data_t *lr) {
    return (int32_t)lr->sizes.flip;
}

/* Capture timestamp as Unix seconds (UTC). LibRaw stores a `time_t`
 * which is 64-bit on modern macOS/Linux; we widen to int64_t for
 * explicit portability across the FFI boundary. */
int64_t ph_libraw_timestamp(libraw_data_t *lr) {
    return (int64_t)libraw_get_imgother(lr)->timestamp;
}

/* === Decode accessors (RawImage inputs) ========================== */

/* CFA mosaic bitmask. Use LIBRAW_COLOR(filters, row, col) on the
 * Rust side to discriminate CfaPattern variants. */
uint32_t ph_libraw_filters(libraw_data_t *lr) {
    return (uint32_t)libraw_get_iparams(lr)->filters;
}

/* Global black level (LSB of the sensor's dark-frame baseline). */
int32_t ph_libraw_black(libraw_data_t *lr) {
    return (int32_t)lr->color.black;
}

/* Pointer to the raw Bayer-pattern u16 sensor buffer, populated by
 * libraw_unpack(). NULL if unpack failed or the format is not Bayer.
 * Owned by LibRaw; the Rust caller must memcpy before libraw_close(). */
const uint16_t *ph_libraw_raw_image(libraw_data_t *lr) {
    return lr->rawdata.raw_image;
}

/* Size of the raw_image buffer in u16 samples (= raw_width * raw_height). */
uint64_t ph_libraw_raw_image_samples(libraw_data_t *lr) {
    return (uint64_t)libraw_get_raw_width(lr) * (uint64_t)libraw_get_raw_height(lr);
}

/* === Processed-image accessors (RgbImage inputs — D1e) =========== */
/* These operate on the libraw_processed_image_t struct returned by
 * libraw_dcraw_make_mem_image(). The Rust caller must free that pointer
 * via libraw_dcraw_clear_mem() after copying the data. */

/* Width of the demosaiced image in pixels. */
uint32_t ph_libraw_img_width(libraw_processed_image_t *img) {
    return (uint32_t)img->width;
}

/* Height of the demosaiced image in pixels. */
uint32_t ph_libraw_img_height(libraw_processed_image_t *img) {
    return (uint32_t)img->height;
}

/* Bits per sample: 8 for 8-bit output (default), 16 for 16-bit. */
uint16_t ph_libraw_img_bits(libraw_processed_image_t *img) {
    return (uint16_t)img->bits;
}

/* Number of colour channels: 3 for RGB (normal), 4 for RGBA. */
uint16_t ph_libraw_img_colors(libraw_processed_image_t *img) {
    return (uint16_t)img->colors;
}

/* Size of the pixel data buffer in bytes (= width * height * colors * bits/8). */
uint32_t ph_libraw_img_data_size(libraw_processed_image_t *img) {
    return (uint32_t)img->data_size;
}

/* Pointer to the pixel data buffer, row-major, LibRaw-owned.
 * The Rust caller must memcpy before calling libraw_dcraw_clear_mem(). */
unsigned char *ph_libraw_img_data(libraw_processed_image_t *img) {
    return img->data;
}

#ifndef ARCHNAV_MIME_ICON_H
#define ARCHNAV_MIME_ICON_H

#include <cstddef>

// C interface for Rust - resolves a theme icon name for a file
extern "C" {
    /**
     * Write the icon theme name for a file's MIME type into `out`, chosen from
     * the file name alone (no filesystem access, so NAS paths cost nothing).
     * Falls back to the type's generic icon, then "unknown", when the theme
     * lacks the specific one.
     * @return The name's length in bytes, or 0 if it did not fit.
     */
    size_t mime_icon_name(const char* file_name, char* out, size_t out_len);
}

#endif // ARCHNAV_MIME_ICON_H

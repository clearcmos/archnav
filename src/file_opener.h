#ifndef ARCHNAV_FILE_OPENER_H
#define ARCHNAV_FILE_OPENER_H

// C interface for Rust - opens files with proper Wayland focus handling
extern "C" {
    /**
     * Open a file using KIO::OpenUrlJob with proper Wayland activation token.
     * This ensures the opened application receives focus (same as Dolphin).
     * @param path Absolute path to the file to open
     */
    void kio_open_file(const char* path);
}

#endif // ARCHNAV_FILE_OPENER_H

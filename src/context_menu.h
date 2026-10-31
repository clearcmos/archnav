#ifndef ARCHNAV_CONTEXT_MENU_H
#define ARCHNAV_CONTEXT_MENU_H

#include <QPoint>
#include <QStringList>
#include <cstddef>

/**
 * Context menu handler for file operations.
 * Uses Qt QMenu and calls KDE tools (kioclient, dolphin) for operations.
 */
class ContextMenuHandler
{
public:
    ContextMenuHandler();
    ~ContextMenuHandler();

    /**
     * Show a context menu for the given files at the specified position.
     * Actions that need one item (Rename, Open Terminal Here, Open as
     * Administrator) appear only for a single file.
     * @param filePaths Absolute paths of the selected files and directories
     * @param globalPos Global screen position for the menu
     */
    void showContextMenu(const QStringList &filePaths, const QPoint &globalPos);
};

// C interface for Rust. `paths` is an array of `count` UTF-8 paths.
extern "C" {
    ContextMenuHandler* create_context_menu_handler();
    void show_context_menu(ContextMenuHandler* handler, const char* const* paths, size_t count, int x, int y);
    void destroy_context_menu_handler(ContextMenuHandler* handler);

    /** Put the files on the clipboard, as Copy in the menu does. */
    void copy_files_to_clipboard(const char* const* paths, size_t count);
    /** Put plain text on the clipboard. */
    void copy_text_to_clipboard(const char* text);
    /**
     * Drag the files out of the window, to Dolphin, a browser, or any other
     * drop target. Blocks until the drop or cancel.
     */
    void start_file_drag(const char* const* paths, size_t count);
}

#endif // ARCHNAV_CONTEXT_MENU_H

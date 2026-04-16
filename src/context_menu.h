#ifndef ARCHNAV_CONTEXT_MENU_H
#define ARCHNAV_CONTEXT_MENU_H

#include <QString>
#include <QPoint>

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
     * Show a context menu for the given file path at the specified position.
     * @param filePath Absolute path to the file or directory
     * @param globalPos Global screen position for the menu
     */
    void showContextMenu(const QString &filePath, const QPoint &globalPos);
};

// C interface for Rust
extern "C" {
    ContextMenuHandler* create_context_menu_handler();
    void show_context_menu(ContextMenuHandler* handler, const char* path, int x, int y, void* window);
    void destroy_context_menu_handler(ContextMenuHandler* handler);
}

#endif // ARCHNAV_CONTEXT_MENU_H

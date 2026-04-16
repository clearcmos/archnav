#ifndef ARCHNAV_SYSTEM_TRAY_H
#define ARCHNAV_SYSTEM_TRAY_H

#include <QSystemTrayIcon>
#include <QMenu>
#include <QString>

class SystemTrayHandler;

/**
 * System tray icon for ArchNav.
 * - Tray icon always visible when app running
 * - Left-click toggles window visibility
 * - Right-click shows menu with "Show ArchNav" / "Configure Shortcut..." / "Exit"
 *
 * For global hotkey: Users should set up a KDE custom shortcut
 * that runs "archnav --toggle"
 */
class SystemTray
{
public:
    using ToggleCallback = void (*)();
    using ExitCallback = void (*)();

    SystemTray(ToggleCallback toggleCb, ExitCallback exitCb);
    ~SystemTray();

    /**
     * Set the preferred hotkey (for reference only).
     * Actual hotkey must be configured via KDE System Settings.
     */
    void setHotkey(const QString &hotkey);

    /**
     * Update the tray icon to reflect window visibility state.
     */
    void setWindowVisible(bool visible);

    // Called by helper
    void onTrayActivated(QSystemTrayIcon::ActivationReason reason);
    void onShowTriggered();
    void onExitTriggered();

private:
    void setupTray();
    void setupGlobalShortcut();

    QSystemTrayIcon *m_trayIcon;
    QMenu *m_trayMenu;
    QAction *m_showAction;
    QAction *m_exitAction;
    SystemTrayHandler *m_handler;

    ToggleCallback m_toggleCallback;
    ExitCallback m_exitCallback;

    QString m_hotkey;
};

// C interface for Rust
extern "C" {
    SystemTray* create_system_tray(void (*toggle_cb)(), void (*exit_cb)());
    void system_tray_set_hotkey(SystemTray* tray, const char* hotkey);
    void system_tray_set_window_visible(SystemTray* tray, bool visible);
    void destroy_system_tray(SystemTray* tray);
}

#endif // ARCHNAV_SYSTEM_TRAY_H

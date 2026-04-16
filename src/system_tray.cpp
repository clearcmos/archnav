#include "system_tray.h"

#include <QApplication>
#include <QIcon>
#include <QProcess>
#include <QDebug>

// Helper QObject for signal connections (since SystemTray is not a QObject)
class SystemTrayHandler : public QObject
{
    Q_OBJECT
public:
    SystemTrayHandler(SystemTray *tray) : m_tray(tray) {}

public slots:
    void onTrayActivated(QSystemTrayIcon::ActivationReason reason) {
        if (m_tray) m_tray->onTrayActivated(reason);
    }
    void onShowTriggered() {
        if (m_tray) m_tray->onShowTriggered();
    }
    void onExitTriggered() {
        if (m_tray) m_tray->onExitTriggered();
    }

private:
    SystemTray *m_tray;
};

// Include MOC output
#include "system_tray.moc"

SystemTray::SystemTray(ToggleCallback toggleCb, ExitCallback exitCb)
    : m_trayIcon(nullptr)
    , m_trayMenu(nullptr)
    , m_showAction(nullptr)
    , m_exitAction(nullptr)
    , m_handler(nullptr)
    , m_toggleCallback(toggleCb)
    , m_exitCallback(exitCb)
{
    m_handler = new SystemTrayHandler(this);
    setupTray();
}

SystemTray::~SystemTray()
{
    delete m_trayMenu;
    delete m_trayIcon;
    delete m_handler;
}

void SystemTray::setupTray()
{
    // Create tray icon
    m_trayIcon = new QSystemTrayIcon();

    // Use a file search icon - fitting for a file navigator
    QIcon icon = QIcon::fromTheme("system-file-manager",
                 QIcon::fromTheme("folder",
                 QIcon::fromTheme("document-open")));
    m_trayIcon->setIcon(icon);
    m_trayIcon->setToolTip("archnav - File Navigator");

    // Create context menu
    m_trayMenu = new QMenu();

    m_showAction = m_trayMenu->addAction(QIcon::fromTheme("window-new"), "Show archnav");
    QObject::connect(m_showAction, &QAction::triggered, m_handler, &SystemTrayHandler::onShowTriggered);

    m_trayMenu->addSeparator();

    QAction *shortcutAction = m_trayMenu->addAction(QIcon::fromTheme("configure-shortcuts"), "Configure Shortcut...");
    QObject::connect(shortcutAction, &QAction::triggered, []() {
        // Open KDE Shortcuts settings (Plasma 6: kcm_keys)
        // User needs to add a custom shortcut that runs "archnav --toggle"
        QProcess::startDetached("systemsettings", {"kcm_keys"});
    });

    m_trayMenu->addSeparator();

    m_exitAction = m_trayMenu->addAction(QIcon::fromTheme("application-exit"), "Exit");
    QObject::connect(m_exitAction, &QAction::triggered, m_handler, &SystemTrayHandler::onExitTriggered);

    m_trayIcon->setContextMenu(m_trayMenu);

    // Connect left-click to toggle
    QObject::connect(m_trayIcon, &QSystemTrayIcon::activated,
                     m_handler, &SystemTrayHandler::onTrayActivated);

    // Show the tray icon
    m_trayIcon->show();

    qDebug() << "[SystemTray] Tray icon created and shown";
}

void SystemTray::setHotkey(const QString &hotkey)
{
    // Store preferred hotkey for reference, but don't try to register via portal
    // The GlobalShortcuts portal is complex and has known issues on KDE Plasma.
    // Instead, users should set up a custom shortcut in KDE System Settings
    // that runs "archnav --toggle"
    m_hotkey = hotkey;
    qDebug() << "[SystemTray] Preferred hotkey:" << hotkey;
    qDebug() << "[SystemTray] To enable: KDE Settings -> Shortcuts -> Custom Shortcuts -> Add 'archnav --toggle'";
}

void SystemTray::setWindowVisible(bool visible)
{
    if (m_showAction) {
        m_showAction->setText(visible ? "Hide archnav" : "Show archnav");
    }
}

void SystemTray::onTrayActivated(QSystemTrayIcon::ActivationReason reason)
{
    if (reason == QSystemTrayIcon::Trigger ||
        reason == QSystemTrayIcon::DoubleClick) {
        // Left-click or double-click: toggle window
        if (m_toggleCallback) {
            m_toggleCallback();
        }
    }
}

void SystemTray::onShowTriggered()
{
    if (m_toggleCallback) {
        m_toggleCallback();
    }
}

void SystemTray::onExitTriggered()
{
    if (m_exitCallback) {
        m_exitCallback();
    }
}

void SystemTray::setupGlobalShortcut()
{
    // GlobalShortcuts portal disabled - too complex and buggy
    // Users should set up a custom KDE shortcut instead
}

// C interface implementation
extern "C" {

SystemTray* create_system_tray(void (*toggle_cb)(), void (*exit_cb)())
{
    return new SystemTray(toggle_cb, exit_cb);
}

void system_tray_set_hotkey(SystemTray* tray, const char* hotkey)
{
    if (tray && hotkey) {
        tray->setHotkey(QString::fromUtf8(hotkey));
    }
}

void system_tray_set_window_visible(SystemTray* tray, bool visible)
{
    if (tray) {
        tray->setWindowVisible(visible);
    }
}

void destroy_system_tray(SystemTray* tray)
{
    delete tray;
}

} // extern "C"

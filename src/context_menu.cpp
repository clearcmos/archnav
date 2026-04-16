#include "context_menu.h"

#include <QMenu>
#include <QAction>
#include <QApplication>
#include <QClipboard>
#include <QMimeData>
#include <QUrl>
#include <QFileInfo>
#include <QMimeDatabase>
#include <QProcess>
#include <QIcon>
#include <QDir>
#include <QStandardPaths>
#include <QDBusInterface>
#include <QDBusConnection>

// KDE Frameworks - same app discovery as Dolphin (instant, uses cached data)
#include <KApplicationTrader>
#include <KService>

ContextMenuHandler::ContextMenuHandler()
{
}

ContextMenuHandler::~ContextMenuHandler()
{
}

void ContextMenuHandler::showContextMenu(const QString &filePath, const QPoint &globalPos)
{
    QFileInfo fileInfo(filePath);
    if (!fileInfo.exists()) {
        return;
    }

    QUrl fileUrl = QUrl::fromLocalFile(filePath);
    QMimeDatabase mimeDb;
    QMimeType mimeType = mimeDb.mimeTypeForFile(filePath);
    QString mimeTypeName = mimeType.name();

    QMenu menu;

    // === Open with default app (instant - uses KDE cached service data) ===
    KService::Ptr defaultService = KApplicationTrader::preferredService(mimeTypeName);

    if (defaultService) {
        QString openText = QString("Open with %1").arg(defaultService->name());
        QString iconName = defaultService->icon();
        QString desktopPath = defaultService->entryPath();
        QAction *openAction = menu.addAction(
            QIcon::fromTheme(iconName.isEmpty() ? "document-open" : iconName),
            openText);
        QObject::connect(openAction, &QAction::triggered, [filePath, desktopPath]() {
            QProcess::startDetached("kioclient", {"exec", desktopPath, filePath});
        });
    } else if (fileInfo.isDir()) {
        QAction *openAction = menu.addAction(QIcon::fromTheme("folder-open"), "Open");
        QObject::connect(openAction, &QAction::triggered, [filePath]() {
            QProcess::startDetached("dolphin", {filePath});
        });
    } else {
        QAction *openAction = menu.addAction(QIcon::fromTheme("document-open"), "Open");
        QObject::connect(openAction, &QAction::triggered, [filePath]() {
            QProcess::startDetached("xdg-open", {filePath});
        });
    }

    // === Open With submenu ===
    // Use KApplicationTrader - same discovery mechanism as Dolphin (instant)
    QMenu *openWithMenu = menu.addMenu(QIcon::fromTheme("applications-other"), "Open With");

    // Query applications that can handle this MIME type (exactly like Dolphin)
    KService::List services = KApplicationTrader::queryByMimeType(mimeTypeName);

    // Skip the default app (already shown above) and add the rest
    QString defaultDesktopName = defaultService ? defaultService->desktopEntryName() : QString();
    for (const KService::Ptr &service : services) {
        if (service->desktopEntryName() == defaultDesktopName) {
            continue;  // Skip default app, already in main menu
        }

        QString serviceName = service->name();
        QString serviceIcon = service->icon();
        QString desktopPath = service->entryPath();

        QAction *appAction = openWithMenu->addAction(
            QIcon::fromTheme(serviceIcon.isEmpty() ? "application-x-executable" : serviceIcon),
            serviceName);

        QObject::connect(appAction, &QAction::triggered, [filePath, desktopPath]() {
            // Use kioclient to launch via .desktop file (proper KDE way)
            QProcess::startDetached("kioclient", {"exec", desktopPath, filePath});
        });
    }

    if (openWithMenu->isEmpty()) {
        openWithMenu->addAction("No applications found")->setEnabled(false);
    }

    openWithMenu->addSeparator();
    QAction *chooseAction = openWithMenu->addAction("Other Application...");
    QObject::connect(chooseAction, &QAction::triggered, [filePath]() {
        // Use KDE's open-with dialog
        QProcess::startDetached("kioclient", {"openWith", filePath});
    });

    menu.addSeparator();

    // === Cut ===
    QAction *cutAction = menu.addAction(QIcon::fromTheme("edit-cut"), "Cut");
    cutAction->setShortcut(QKeySequence::Cut);
    QObject::connect(cutAction, &QAction::triggered, [fileUrl]() {
        QMimeData *mimeData = new QMimeData();
        mimeData->setUrls({fileUrl});
        mimeData->setData("application/x-kde-cutselection", "1");
        QApplication::clipboard()->setMimeData(mimeData);
    });

    // === Copy ===
    QAction *copyAction = menu.addAction(QIcon::fromTheme("edit-copy"), "Copy");
    copyAction->setShortcut(QKeySequence::Copy);
    QObject::connect(copyAction, &QAction::triggered, [fileUrl]() {
        QMimeData *mimeData = new QMimeData();
        mimeData->setUrls({fileUrl});
        QApplication::clipboard()->setMimeData(mimeData);
    });

    // === Copy Location ===
    QAction *copyPathAction = menu.addAction(QIcon::fromTheme("edit-copy-path"), "Copy Location");
    copyPathAction->setShortcut(QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_C));
    QObject::connect(copyPathAction, &QAction::triggered, [filePath]() {
        QApplication::clipboard()->setText(filePath);
    });

    // === Duplicate Here ===
    if (!fileInfo.isDir()) {
        QAction *duplicateAction = menu.addAction(QIcon::fromTheme("edit-duplicate"), "Duplicate Here");
        duplicateAction->setShortcut(QKeySequence(Qt::CTRL | Qt::Key_D));
        QObject::connect(duplicateAction, &QAction::triggered, [filePath]() {
            QString baseName = QFileInfo(filePath).completeBaseName();
            QString suffix = QFileInfo(filePath).suffix();
            QString dir = QFileInfo(filePath).absolutePath();
            QString newName = baseName + " (copy)";
            if (!suffix.isEmpty()) {
                newName += "." + suffix;
            }
            QString dest = dir + "/" + newName;
            QFile::copy(filePath, dest);
        });
    }

    // === Rename ===
    QAction *renameAction = menu.addAction(QIcon::fromTheme("edit-rename"), "Rename...");
    renameAction->setShortcut(Qt::Key_F2);
    QObject::connect(renameAction, &QAction::triggered, [filePath]() {
        // Open dolphin with file selected for rename
        QProcess::startDetached("dolphin", {"--select", filePath});
    });

    menu.addSeparator();

    // === Delete (permanent) ===
    QAction *deleteAction = menu.addAction(QIcon::fromTheme("edit-delete"), "Delete");
    deleteAction->setShortcut(QKeySequence(Qt::SHIFT | Qt::Key_Delete));
    QObject::connect(deleteAction, &QAction::triggered, [filePath, fileInfo]() {
        if (fileInfo.isDir()) {
            QDir(filePath).removeRecursively();
        } else {
            QFile::remove(filePath);
        }
    });

    // === Open Terminal Here ===
    QString terminalDir = fileInfo.isDir() ? filePath : fileInfo.absolutePath();
    QAction *terminalAction = menu.addAction(QIcon::fromTheme("utilities-terminal"), "Open Terminal Here");
    terminalAction->setShortcut(QKeySequence(Qt::ALT | Qt::SHIFT | Qt::Key_F4));
    QObject::connect(terminalAction, &QAction::triggered, [terminalDir]() {
        QProcess::startDetached("konsole", {"--workdir", terminalDir});
    });

    // === Move to New Folder ===
    QAction *moveToFolderAction = menu.addAction(QIcon::fromTheme("folder-new"), "Move to New Folder...");
    QObject::connect(moveToFolderAction, &QAction::triggered, [filePath, fileInfo]() {
        QString dir = fileInfo.absolutePath();
        QString newDir = dir + "/New Folder";
        int i = 1;
        while (QDir(newDir).exists()) {
            newDir = dir + QString("/New Folder (%1)").arg(i++);
        }
        QDir().mkdir(newDir);
        QString dest = newDir + "/" + fileInfo.fileName();
        QFile::rename(filePath, dest);
        // Open the new folder in dolphin
        QProcess::startDetached("dolphin", {newDir});
    });

    menu.addSeparator();

    // === Move to Trash ===
    QAction *trashAction = menu.addAction(QIcon::fromTheme("user-trash"), "Move to Trash");
    QObject::connect(trashAction, &QAction::triggered, [filePath]() {
        QProcess::startDetached("kioclient", {"move", filePath, "trash:/"});
    });

    menu.addSeparator();

    // === Open as Administrator ===
    QAction *adminAction = menu.addAction(QIcon::fromTheme("dialog-password"), "Open as Administrator");
    QObject::connect(adminAction, &QAction::triggered, [filePath, fileInfo]() {
        if (fileInfo.isDir()) {
            QProcess::startDetached("pkexec", {"dolphin", filePath});
        } else {
            QProcess::startDetached("pkexec", {"xdg-open", filePath});
        }
    });

    // === Compress submenu ===
    QMenu *compressMenu = menu.addMenu(QIcon::fromTheme("archive-insert"), "Compress");

    QAction *compressZip = compressMenu->addAction("Create ZIP Archive");
    QObject::connect(compressZip, &QAction::triggered, [filePath]() {
        QProcess::startDetached("ark", {"--add", "--changetofirstpath", filePath});
    });

    QAction *compressTar = compressMenu->addAction("Create TAR.GZ Archive");
    QObject::connect(compressTar, &QAction::triggered, [filePath]() {
        QProcess::startDetached("ark", {"--add", "--changetofirstpath", "--mimetypes", "application/x-compressed-tar", filePath});
    });

    menu.addSeparator();

    // === Properties ===
    QAction *propsAction = menu.addAction(QIcon::fromTheme("document-properties"), "Properties");
    propsAction->setShortcut(QKeySequence(Qt::ALT | Qt::Key_Return));
    QObject::connect(propsAction, &QAction::triggered, [fileUrl]() {
        // Use freedesktop FileManager1 DBus interface - works for all paths including network mounts
        QDBusInterface iface("org.freedesktop.FileManager1",
                             "/org/freedesktop/FileManager1",
                             "org.freedesktop.FileManager1",
                             QDBusConnection::sessionBus());
        if (iface.isValid()) {
            iface.call("ShowItemProperties", QStringList{fileUrl.toString()}, QString());
        }
    });

    // Show the menu
    menu.exec(globalPos);
}

// C interface for Rust
extern "C" {

ContextMenuHandler* create_context_menu_handler()
{
    return new ContextMenuHandler();
}

void show_context_menu(ContextMenuHandler* handler, const char* path, int x, int y, void* window)
{
    Q_UNUSED(window);
    if (handler && path) {
        handler->showContextMenu(
            QString::fromUtf8(path),
            QPoint(x, y)
        );
    }
}

void destroy_context_menu_handler(ContextMenuHandler* handler)
{
    delete handler;
}

} // extern "C"

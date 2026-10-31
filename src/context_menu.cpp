#include "context_menu.h"

#include <QMenu>
#include <QAction>
#include <QApplication>
#include <QClipboard>
#include <QDrag>
#include <QMimeData>
#include <QUrl>
#include <QFileInfo>
#include <QMimeDatabase>
#include <QPainter>
#include <QPointer>
#include <QWindow>
#include <QProcess>
#include <QIcon>
#include <QDir>
#include <QStandardPaths>
#include <QDBusInterface>
#include <QDBusConnection>
#include <QMessageBox>

// KDE Frameworks - same app discovery as Dolphin (instant, uses cached data)
#include <KApplicationTrader>
#include <KService>
#include <KIO/ApplicationLauncherJob>
#include <KIO/JobUiDelegate>
#include <KIO/JobUiDelegateFactory>

namespace {

QStringList pathList(const char* const* paths, size_t count)
{
    QStringList list;
    for (size_t i = 0; i < count; ++i) {
        if (paths[i]) {
            list << QString::fromUtf8(paths[i]);
        }
    }
    return list;
}

QList<QUrl> urlList(const QStringList &paths)
{
    QList<QUrl> urls;
    for (const QString &path : paths) {
        urls << QUrl::fromLocalFile(path);
    }
    return urls;
}

QStringList urlStrings(const QStringList &paths)
{
    QStringList urls;
    for (const QString &path : paths) {
        urls << QUrl::fromLocalFile(path).toString();
    }
    return urls;
}

// Start `service` (or, with none, KDE's Open With dialog) on the files. The
// launcher job hands the app a Wayland activation token, as Dolphin does.
void launch(const KService::Ptr &service, const QStringList &paths)
{
    auto *job = service ? new KIO::ApplicationLauncherJob(service) : new KIO::ApplicationLauncherJob();
    job->setUrls(urlList(paths));
    // KIOWidgets registers the delegate factory, which brings the Open With
    // dialog, when it loads. Naming one of its symbols here also keeps the
    // linker from dropping the library as unused.
    KJobUiDelegate *delegate = KIO::createDefaultJobUiDelegate(KJobUiDelegate::AutoHandlingEnabled, nullptr);
    if (!delegate) {
        qWarning("No KIO job delegate: %s is not loaded", KIO::JobUiDelegate::staticMetaObject.className());
    }
    job->setUiDelegate(delegate);
    job->start();
}

// The MIME type the files share, or the closest common one for a mix
QString commonMimeType(const QStringList &paths)
{
    QMimeDatabase mimeDb;
    QString common;
    bool allDirs = true;
    for (const QString &path : paths) {
        const QString name = mimeDb.mimeTypeForFile(path).name();
        allDirs = allDirs && QFileInfo(path).isDir();
        if (common.isEmpty()) {
            common = name;
        } else if (common != name) {
            return allDirs ? QStringLiteral("inode/directory") : QStringLiteral("application/octet-stream");
        }
    }
    return common;
}

QMimeData *fileMimeData(const QStringList &paths)
{
    auto *mimeData = new QMimeData();
    mimeData->setUrls(urlList(paths));
    mimeData->setText(paths.join('\n'));
    return mimeData;
}

// The first file's type icon, with the number of files on it when there are
// several, as the drag image
QPixmap dragPixmap(const QStringList &paths, qreal dpr)
{
    const int size = 32;
    const QFileInfo first(paths.first());
    QString iconName = QStringLiteral("folder");
    QString fallback = QStringLiteral("folder");
    if (!first.isDir()) {
        const QMimeType type = QMimeDatabase().mimeTypeForFile(first.fileName(), QMimeDatabase::MatchExtension);
        iconName = type.iconName();
        fallback = type.genericIconName();
    }
    QIcon icon = QIcon::fromTheme(iconName, QIcon::fromTheme(fallback, QIcon::fromTheme("unknown")));
    QPixmap pixmap(QSize(size, size) * dpr);
    pixmap.setDevicePixelRatio(dpr);
    pixmap.fill(Qt::transparent);
    QPainter painter(&pixmap);
    icon.paint(&painter, QRect(0, 0, size, size));
    if (paths.size() > 1) {
        const QString count = paths.size() > 99 ? QStringLiteral("99+") : QString::number(paths.size());
        QFont font = QApplication::font();
        font.setBold(true);
        font.setPixelSize(10);
        painter.setFont(font);
        const int width = qMax(16, QFontMetrics(font).horizontalAdvance(count) + 6);
        const QRect badge(size - width, size - 16, width, 16);
        painter.setRenderHint(QPainter::Antialiasing);
        painter.setPen(Qt::NoPen);
        painter.setBrush(QApplication::palette().highlight());
        painter.drawRoundedRect(badge, 8, 8);
        painter.setPen(QApplication::palette().highlightedText().color());
        painter.drawText(badge, Qt::AlignCenter, count);
    }
    return pixmap;
}

} // namespace

ContextMenuHandler::ContextMenuHandler()
{
}

ContextMenuHandler::~ContextMenuHandler()
{
}

void ContextMenuHandler::showContextMenu(const QStringList &requested, const QPoint &globalPos)
{
    QStringList filePaths;
    for (const QString &path : requested) {
        if (QFileInfo::exists(path)) {
            filePaths << path;
        }
    }
    if (filePaths.isEmpty()) {
        return;
    }

    const bool single = filePaths.size() == 1;
    const QString filePath = filePaths.first();
    const QFileInfo fileInfo(filePath);
    bool allDirs = true;
    bool anyDir = false;
    for (const QString &path : filePaths) {
        const bool isDir = QFileInfo(path).isDir();
        allDirs = allDirs && isDir;
        anyDir = anyDir || isDir;
    }
    const QString mimeTypeName = commonMimeType(filePaths);

    QMenu menu;

    // === Open with default app (instant - uses KDE cached service data) ===
    KService::Ptr defaultService = KApplicationTrader::preferredService(mimeTypeName);

    if (defaultService) {
        QString openText = QString("Open with %1").arg(defaultService->name());
        QString iconName = defaultService->icon();
        QAction *openAction = menu.addAction(
            QIcon::fromTheme(iconName.isEmpty() ? "document-open" : iconName),
            openText);
        QObject::connect(openAction, &QAction::triggered, [filePaths, defaultService]() {
            launch(defaultService, filePaths);
        });
    } else if (allDirs) {
        QAction *openAction = menu.addAction(QIcon::fromTheme("folder-open"), "Open");
        QObject::connect(openAction, &QAction::triggered, [filePaths]() {
            QProcess::startDetached("dolphin", filePaths);
        });
    } else {
        QAction *openAction = menu.addAction(QIcon::fromTheme("document-open"), "Open");
        QObject::connect(openAction, &QAction::triggered, [filePaths]() {
            for (const QString &path : filePaths) {
                QProcess::startDetached("xdg-open", {path});
            }
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

        QString serviceIcon = service->icon();
        QAction *appAction = openWithMenu->addAction(
            QIcon::fromTheme(serviceIcon.isEmpty() ? "application-x-executable" : serviceIcon),
            service->name());

        QObject::connect(appAction, &QAction::triggered, [filePaths, service]() {
            launch(service, filePaths);
        });
    }

    if (openWithMenu->isEmpty()) {
        openWithMenu->addAction("No applications found")->setEnabled(false);
    }

    openWithMenu->addSeparator();
    QAction *chooseAction = openWithMenu->addAction("Other Application...");
    QObject::connect(chooseAction, &QAction::triggered, [filePaths]() {
        // No service: the launcher job asks with KDE's Open With dialog
        launch(KService::Ptr(), filePaths);
    });

    menu.addSeparator();

    // === Cut ===
    QAction *cutAction = menu.addAction(QIcon::fromTheme("edit-cut"), "Cut");
    cutAction->setShortcut(QKeySequence::Cut);
    QObject::connect(cutAction, &QAction::triggered, [filePaths]() {
        QMimeData *mimeData = fileMimeData(filePaths);
        mimeData->setData("application/x-kde-cutselection", "1");
        QApplication::clipboard()->setMimeData(mimeData);
    });

    // === Copy ===
    QAction *copyAction = menu.addAction(QIcon::fromTheme("edit-copy"), "Copy");
    copyAction->setShortcut(QKeySequence::Copy);
    QObject::connect(copyAction, &QAction::triggered, [filePaths]() {
        QApplication::clipboard()->setMimeData(fileMimeData(filePaths));
    });

    // === Copy Location ===
    QAction *copyPathAction = menu.addAction(QIcon::fromTheme("edit-copy-path"), "Copy Location");
    copyPathAction->setShortcut(QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_C));
    QObject::connect(copyPathAction, &QAction::triggered, [filePaths]() {
        QApplication::clipboard()->setText(filePaths.join('\n'));
    });

    // === Duplicate Here ===
    if (!anyDir) {
        QAction *duplicateAction = menu.addAction(QIcon::fromTheme("edit-duplicate"), "Duplicate Here");
        duplicateAction->setShortcut(QKeySequence(Qt::CTRL | Qt::Key_D));
        QObject::connect(duplicateAction, &QAction::triggered, [filePaths]() {
            for (const QString &path : filePaths) {
                const QFileInfo info(path);
                QString newName = info.completeBaseName() + " (copy)";
                if (!info.suffix().isEmpty()) {
                    newName += "." + info.suffix();
                }
                QFile::copy(path, info.absolutePath() + "/" + newName);
            }
        });
    }

    // === Rename ===
    if (single) {
        QAction *renameAction = menu.addAction(QIcon::fromTheme("edit-rename"), "Rename...");
        renameAction->setShortcut(Qt::Key_F2);
        QObject::connect(renameAction, &QAction::triggered, [filePath]() {
            // Open dolphin with file selected for rename
            QProcess::startDetached("dolphin", {"--select", filePath});
        });
    }

    menu.addSeparator();

    // === Delete (permanent) ===
    QAction *deleteAction = menu.addAction(QIcon::fromTheme("edit-delete"), "Delete");
    deleteAction->setShortcut(QKeySequence(Qt::SHIFT | Qt::Key_Delete));
    QObject::connect(deleteAction, &QAction::triggered, [filePaths, fileInfo, single, anyDir]() {
        // Permanent (and for directories recursive) deletion must never be a
        // single misclick away; confirm with No as the default.
        QString what;
        if (single) {
            what = fileInfo.isDir()
                ? QString("the folder \"%1\" and everything inside it").arg(fileInfo.fileName())
                : QString("\"%1\"").arg(fileInfo.fileName());
        } else {
            what = QString("%1 items%2").arg(filePaths.size())
                .arg(anyDir ? ", including folders and everything inside them" : "");
        }
        const QMessageBox::StandardButton answer = QMessageBox::warning(
            nullptr,
            "Delete Permanently",
            QString("Permanently delete %1?\n\nThis cannot be undone.").arg(what),
            QMessageBox::Yes | QMessageBox::No,
            QMessageBox::No);
        if (answer != QMessageBox::Yes) {
            return;
        }
        for (const QString &path : filePaths) {
            if (QFileInfo(path).isDir()) {
                QDir(path).removeRecursively();
            } else {
                QFile::remove(path);
            }
        }
    });

    // === Open Terminal Here ===
    if (single) {
        QString terminalDir = fileInfo.isDir() ? filePath : fileInfo.absolutePath();
        QAction *terminalAction = menu.addAction(QIcon::fromTheme("utilities-terminal"), "Open Terminal Here");
        terminalAction->setShortcut(QKeySequence(Qt::ALT | Qt::SHIFT | Qt::Key_F4));
        QObject::connect(terminalAction, &QAction::triggered, [terminalDir]() {
            QProcess::startDetached("konsole", {"--workdir", terminalDir});
        });
    }

    // === Move to New Folder ===
    // Only for items in one folder, where the new folder is created
    bool sameFolder = true;
    for (const QString &path : filePaths) {
        sameFolder = sameFolder && QFileInfo(path).absolutePath() == fileInfo.absolutePath();
    }
    if (sameFolder) {
        QAction *moveToFolderAction = menu.addAction(QIcon::fromTheme("folder-new"), "Move to New Folder...");
        QObject::connect(moveToFolderAction, &QAction::triggered, [filePaths, fileInfo]() {
            QString dir = fileInfo.absolutePath();
            QString newDir = dir + "/New Folder";
            int i = 1;
            while (QDir(newDir).exists()) {
                newDir = dir + QString("/New Folder (%1)").arg(i++);
            }
            QDir().mkdir(newDir);
            for (const QString &path : filePaths) {
                QFile::rename(path, newDir + "/" + QFileInfo(path).fileName());
            }
            // Open the new folder in dolphin
            QProcess::startDetached("dolphin", {newDir});
        });
    }

    menu.addSeparator();

    // === Move to Trash ===
    QAction *trashAction = menu.addAction(QIcon::fromTheme("user-trash"), "Move to Trash");
    QObject::connect(trashAction, &QAction::triggered, [filePaths]() {
        QProcess::startDetached("kioclient", QStringList{"move"} + filePaths + QStringList{"trash:/"});
    });

    menu.addSeparator();

    // === Open as Administrator ===
    if (single) {
        QAction *adminAction = menu.addAction(QIcon::fromTheme("dialog-password"), "Open as Administrator");
        QObject::connect(adminAction, &QAction::triggered, [filePath, fileInfo]() {
            if (fileInfo.isDir()) {
                QProcess::startDetached("pkexec", {"dolphin", filePath});
            } else {
                QProcess::startDetached("pkexec", {"xdg-open", filePath});
            }
        });
    }

    // === Compress submenu ===
    QMenu *compressMenu = menu.addMenu(QIcon::fromTheme("archive-insert"), "Compress");

    QAction *compressZip = compressMenu->addAction("Create ZIP Archive");
    QObject::connect(compressZip, &QAction::triggered, [filePaths]() {
        QProcess::startDetached("ark", QStringList{"--add", "--changetofirstpath"} + filePaths);
    });

    QAction *compressTar = compressMenu->addAction("Create TAR.GZ Archive");
    QObject::connect(compressTar, &QAction::triggered, [filePaths]() {
        QProcess::startDetached("ark", QStringList{"--add", "--changetofirstpath", "--mimetypes", "application/x-compressed-tar"} + filePaths);
    });

    menu.addSeparator();

    // === Properties ===
    QAction *propsAction = menu.addAction(QIcon::fromTheme("document-properties"), "Properties");
    propsAction->setShortcut(QKeySequence(Qt::ALT | Qt::Key_Return));
    QObject::connect(propsAction, &QAction::triggered, [filePaths]() {
        // Use freedesktop FileManager1 DBus interface - works for all paths including network mounts
        QDBusInterface iface("org.freedesktop.FileManager1",
                             "/org/freedesktop/FileManager1",
                             "org.freedesktop.FileManager1",
                             QDBusConnection::sessionBus());
        if (iface.isValid()) {
            iface.call("ShowItemProperties", urlStrings(filePaths), QString());
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

void show_context_menu(ContextMenuHandler* handler, const char* const* paths, size_t count, int x, int y)
{
    if (handler && paths && count > 0) {
        handler->showContextMenu(pathList(paths, count), QPoint(x, y));
    }
}

void destroy_context_menu_handler(ContextMenuHandler* handler)
{
    delete handler;
}

void copy_files_to_clipboard(const char* const* paths, size_t count)
{
    const QStringList list = pathList(paths, count);
    if (!list.isEmpty()) {
        QApplication::clipboard()->setMimeData(fileMimeData(list));
    }
}

void copy_text_to_clipboard(const char* text)
{
    if (text) {
        QApplication::clipboard()->setText(QString::fromUtf8(text));
    }
}

void start_file_drag(const char* const* paths, size_t count)
{
    const QStringList list = pathList(paths, count);
    if (list.isEmpty()) {
        return;
    }
    // QDrag::exec runs until the drop. Some platforms delete the drag
    // themselves, so free it only if it is still there, as Qt Quick does.
    QPointer<QDrag> drag = new QDrag(qApp);
    drag->setMimeData(fileMimeData(list));
    const QWindow *window = QGuiApplication::focusWindow();
    drag->setPixmap(dragPixmap(list, window ? window->devicePixelRatio() : qApp->devicePixelRatio()));
    drag->exec(Qt::CopyAction | Qt::MoveAction | Qt::LinkAction, Qt::CopyAction);
    if (drag) {
        drag->deleteLater();
    }
}

} // extern "C"

#include "file_opener.h"

#include <QUrl>
#include <QString>
#include <KIO/OpenUrlJob>
#include <KIO/JobUiDelegateFactory>

extern "C" {

void kio_open_file(const char* path)
{
    if (!path) return;

    QUrl url = QUrl::fromLocalFile(QString::fromUtf8(path));

    // KIO::OpenUrlJob automatically handles:
    // - MIME type detection
    // - Finding the right application
    // - XDG activation token for Wayland focus
    // Jobs auto-delete after completion by default
    auto *job = new KIO::OpenUrlJob(url);
    job->setUiDelegate(KIO::createDefaultJobUiDelegate(KJobUiDelegate::AutoHandlingEnabled, nullptr));
    job->start();
}

} // extern "C"

#include "mime_icon.h"

#include <QIcon>
#include <QMimeDatabase>
#include <QString>
#include <cstring>

extern "C" {

size_t mime_icon_name(const char* file_name, char* out, size_t out_len)
{
    if (!file_name || !out || out_len == 0) return 0;

    static const QMimeDatabase db;
    // mimeTypesForFileName matches on the name only. mimeTypeForFile would
    // stat the path and may read its content, which is a network round trip
    // for every result on a NAS mount.
    const QList<QMimeType> types = db.mimeTypesForFileName(QString::fromUtf8(file_name));

    QString icon = QStringLiteral("unknown");
    if (!types.isEmpty()) {
        const QMimeType &type = types.first();
        if (QIcon::hasThemeIcon(type.iconName())) {
            icon = type.iconName();
        } else if (QIcon::hasThemeIcon(type.genericIconName())) {
            icon = type.genericIconName();
        }
    }

    const QByteArray utf8 = icon.toUtf8();
    if (static_cast<size_t>(utf8.size()) >= out_len) return 0;
    std::memcpy(out, utf8.constData(), utf8.size());
    out[utf8.size()] = '\0';
    return utf8.size();
}

} // extern "C"

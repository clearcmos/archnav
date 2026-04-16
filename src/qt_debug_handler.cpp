#include <QtGlobal>
#include <QString>
#include <QByteArray>
#include <cstdio>

static void customMessageHandler(QtMsgType type, const QMessageLogContext &context, const QString &msg)
{
    QByteArray localMsg = msg.toLocal8Bit();
    const char *file = context.file ? context.file : "";
    const char *function = context.function ? context.function : "";

    switch (type) {
    case QtDebugMsg:
        fprintf(stderr, "[Qt DEBUG] %s (%s:%u, %s)\n", localMsg.constData(), file, context.line, function);
        break;
    case QtInfoMsg:
        fprintf(stderr, "[Qt INFO] %s (%s:%u, %s)\n", localMsg.constData(), file, context.line, function);
        break;
    case QtWarningMsg:
        fprintf(stderr, "[Qt WARNING] %s (%s:%u, %s)\n", localMsg.constData(), file, context.line, function);
        break;
    case QtCriticalMsg:
        fprintf(stderr, "[Qt CRITICAL] %s (%s:%u, %s)\n", localMsg.constData(), file, context.line, function);
        break;
    case QtFatalMsg:
        fprintf(stderr, "[Qt FATAL] %s (%s:%u, %s)\n", localMsg.constData(), file, context.line, function);
        abort();
    }
    fflush(stderr);
}

extern "C" void install_qt_debug_handler()
{
    fprintf(stderr, "[qt_debug_handler] Installing custom Qt message handler\n");
    fflush(stderr);
    qInstallMessageHandler(customMessageHandler);
}

#include <QApplication>
#include <cstdlib>

// Global argc storage (Qt requires argc to stay valid)
static int g_argc = 1;
static char g_arg0[] = "archnav";
static char* g_argv[] = { g_arg0, nullptr };

// Global QApplication pointer
static QApplication* g_app = nullptr;

extern "C" {

// Create QApplication (must be called before any Qt widgets/QML)
void create_qapplication()
{
    if (!g_app) {
        g_app = new QApplication(g_argc, g_argv);
    }
}

// Run the Qt event loop
int run_qapplication()
{
    if (g_app) {
        return g_app->exec();
    }
    return 1;
}

// Clean up QApplication
void destroy_qapplication()
{
    delete g_app;
    g_app = nullptr;
}

// Quit the application (exits event loop)
void quit_qapplication()
{
    if (g_app) {
        g_app->quit();
    }
}

} // extern "C"

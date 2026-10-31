// Real Qt widget/palette probe. Runs on the private offscreen platform; no user display.
#include <QApplication>
#include <QFile>
#include <QIcon>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QTimer>
#include <QVBoxLayout>
#include <QWidget>

int main(int argc, char **argv) {
    QApplication app(argc, argv);
    if (argc != 3) return 2;
    QWidget window;
    window.setAutoFillBackground(true);
    window.setWindowTitle("Decebal Qt theme QA");
    auto *layout = new QVBoxLayout(&window);
    layout->addWidget(new QLabel("Readable Qt body text"));
    layout->addWidget(new QLineEdit("Editable text"));
    layout->addWidget(new QPushButton(QIcon::fromTheme("folder"), "Open folder"));
    auto *disabled = new QPushButton("Disabled action");
    disabled->setEnabled(false);
    layout->addWidget(disabled);
    auto *list = new QListWidget;
    list->addItems({"Selected place", "Documents", "Downloads"});
    list->setCurrentRow(0);
    layout->addWidget(list);
    window.resize(800, 500);
    window.show();
    QTimer::singleShot(300, [&] {
        QJsonObject result;
        const QPalette p = app.palette();
        const auto color = [&](QPalette::ColorGroup group, QPalette::ColorRole role) {
            return p.color(group, role).name();
        };
        result["window"] = color(QPalette::Active, QPalette::Window);
        result["base"] = color(QPalette::Active, QPalette::Base);
        result["button"] = color(QPalette::Active, QPalette::Button);
        result["text"] = color(QPalette::Active, QPalette::Text);
        result["highlight"] = color(QPalette::Active, QPalette::Highlight);
        result["highlight_text"] = color(QPalette::Active, QPalette::HighlightedText);
        result["disabled_text"] = color(QPalette::Disabled, QPalette::Text);
        result["icon_theme"] = QIcon::themeName();
        result["folder_icon"] = QIcon::hasThemeIcon("folder");
        result["qt_version"] = qVersion();
        QFile mappings("/proc/self/maps");
        if (mappings.open(QIODevice::ReadOnly)) {
            result["qt6ct_loaded"] = mappings.readAll().contains(qgetenv("QT_PLUGIN_PATH") + "/platformthemes/libqt6ct.so");
        }
        const bool captured = window.grab().save(QString::fromLocal8Bit(argv[2]));
        QFile out(QString::fromLocal8Bit(argv[1]));
        if (!captured || !out.open(QIODevice::WriteOnly)) { app.exit(3); return; }
        out.write(QJsonDocument(result).toJson());
        out.close();
        app.quit();
    });
    return app.exec();
}

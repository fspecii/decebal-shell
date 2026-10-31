// Real Qt exporter for the app-menu tests: a QMainWindow whose native menu bar Qt exports through
// com.canonical.AppMenu.Registrar + com.canonical.dbusmenu (QDBusMenuBar). Every triggered or
// toggled action is appended to $APPMENU_RECORD as "<event> <objectName>"; the window id goes to
// $APPMENU_READY once the window is mapped. Built by tests/e2e/appmenu-real.sh, never installed.
#include <QAction>
#include <QActionGroup>
#include <QApplication>
#include <QFile>
#include <QMainWindow>
#include <QMenu>
#include <QMenuBar>
#include <QTimer>

static void record(const QString &line) {
    QFile f(qEnvironmentVariable("APPMENU_RECORD"));
    if (f.open(QIODevice::Append | QIODevice::Text)) {
        f.write((line + "\n").toUtf8());
    }
}

static QAction *act(QMenu *m, const char *name, const QString &text) {
    QAction *a = m->addAction(text);
    a->setObjectName(name);
    QObject::connect(a, &QAction::triggered, [a](bool checked) { record(QStringLiteral("triggered %1 %2").arg(a->objectName()).arg(checked ? 1 : 0)); });
    return a;
}

int main(int argc, char **argv) {
    QApplication app(argc, argv);
    QMainWindow w;
    w.setWindowTitle("Decebal AppMenu Qt fixture");
    QMenu *file = w.menuBar()->addMenu("&File");
    file->setObjectName("file");
    act(file, "open", "&Open…")->setShortcut(QKeySequence("Ctrl+O"));
    file->addSeparator();
    QAction *wrap = act(file, "wrap", "Word &Wrap");
    wrap->setCheckable(true);
    wrap->setChecked(true);
    QActionGroup *group = new QActionGroup(&w);
    QAction *small = act(file, "small", "Small");
    QAction *large = act(file, "large", "Large");
    for (QAction *a : {small, large}) {
        a->setCheckable(true);
        group->addAction(a);
    }
    large->setChecked(true);
    QMenu *recent = file->addMenu("&Recent");
    recent->setObjectName("recent");
    act(recent, "notes", "notes.txt");
    QAction *hidden = act(file, "hidden", "Hidden");
    hidden->setVisible(false);
    QMenu *edit = w.menuBar()->addMenu("&Edit");
    edit->setObjectName("edit");
    act(edit, "undo", "&Undo")->setEnabled(false);
    act(edit, "redo", "&Redo");
    // A live change after start-up: Redo becomes disabled and a new item appears (LayoutUpdated/ItemsPropertiesUpdated).
    QObject::connect(edit, &QMenu::aboutToShow, [edit]() {
        record("aboutToShow edit");
        if (!edit->findChild<QAction *>("late")) {
            act(edit, "late", "Late Item");
        }
    });
    w.resize(320, 200);
    w.show();
    QTimer::singleShot(300, [&w]() {
        QFile f(qEnvironmentVariable("APPMENU_READY"));
        if (f.open(QIODevice::WriteOnly | QIODevice::Text)) {
            f.write(QString::number(w.winId()).toUtf8());
        }
    });
    return app.exec();
}

// SPDX-License-Identifier: MIT
// KDE's data-control clipboard can publish the next offer while another app has focus.
#include <KSystemClipboard>
#include <QGuiApplication>
#include <QMimeData>
#include <QFile>
#include <QFileInfo>
#include <QDir>
#include <QJsonDocument>
#include <QJsonArray>
#include <QJsonObject>
#include <QSocketNotifier>
#include <QTimer>
#include <cstdio>
#include <functional>
#include <memory>
#include <unistd.h>

struct Entry { QString mime; QByteArray data; };
static void event(const QString &text) { auto b=text.toUtf8(); std::fwrite(b.constData(),1,b.size(),stdout); std::fputc('\n',stdout); std::fflush(stdout); }
class Offer : public QMimeData {
public:
    Entry entry;
    std::function<void()> consumed;
    mutable bool requested=false;
    bool sensitive=true;
    QStringList formats() const override {
        QStringList result{entry.mime};
        if (sensitive) result << QStringLiteral("x-kde-passwordManagerHint");
        if (entry.mime.startsWith("text/")) result << QStringLiteral("text/plain");
        return result;
    }
protected:
    QVariant retrieveData(const QString &mime,QMetaType) const override {
        if (mime==QStringLiteral("x-kde-passwordManagerHint")) return QByteArrayLiteral("secret");
        if (mime!=entry.mime && !(entry.mime.startsWith("text/") && mime==QStringLiteral("text/plain"))) return {};
        if (!requested) { requested=true; consumed(); }
        return entry.data;
    }
};
int main(int argc,char **argv) {
    QGuiApplication app(argc,argv);app.setQuitOnLastWindowClosed(false);
    const bool restore=argc==3 && QByteArray(argv[2])=="--restore";
    if (argc!=2 && !restore) return 2;
    QFile manifest(QString::fromLocal8Bit(argv[1]));
    if (!manifest.open(QIODevice::ReadOnly) || manifest.size()>65536) return 2;
    auto document=QJsonDocument::fromJson(manifest.readAll());
    auto array=document.array();
    if (array.isEmpty() || array.size()>50) return 2;
    QVector<Entry> entries; qint64 total=0;
    auto directory=QFileInfo(manifest).absoluteDir();
    for (int i=0;i<array.size();i++) {
        auto mime=array[i].toString();
        if (mime!="text/plain;charset=utf-8" && mime!="image/png") return 2;
        QFile file(directory.filePath(QString::number(i)));
        if (!file.open(QIODevice::ReadOnly) || file.size()>67108864 || (total+=file.size())>67108864) return 2;
        entries.push_back({mime,file.readAll()});
    }
    auto clipboard=KSystemClipboard::instance();
    int index=0; bool running=true; bool publishing=false; Offer *current=nullptr;
    std::function<void()> publish;
    publish=[&] {
        const int generation=index;
        auto offer=new Offer; offer->entry=entries[index];offer->sensitive=!restore;
        offer->consumed=[&,generation] {
            if (restore) return;
            QTimer::singleShot(150,&app,[&,generation] {
                if (!running || generation!=index) return;
                if (index+1<entries.size()) { index++;publish();event(QStringLiteral("advance %1").arg(index)); }
                else {running=false;event(QStringLiteral("done"));}
            });
        };
        current=offer;publishing=true;
        clipboard->setMimeData(offer,QClipboard::Clipboard);
        publishing=false;
    };
    QObject::connect(clipboard,&KSystemClipboard::changed,&app,[&](QClipboard::Mode mode) {
        if (!current || mode!=QClipboard::Clipboard || publishing) return;
        if (clipboard->mimeData(mode)!=current) {running=false;event(QStringLiteral("cancelled"));app.quit();}
    });
    QSocketNotifier input(STDIN_FILENO,QSocketNotifier::Read,&app);
    QObject::connect(&input,&QSocketNotifier::activated,&app,[&] {
        char buffer[32];auto n=::read(STDIN_FILENO,buffer,sizeof(buffer));
        if (n<=0) {input.setEnabled(false);if (!restore) app.quit();return;}
        running=false;event(QStringLiteral("stopped"));
    });
    // QWaylandClientExtension binds the background data-control manager asynchronously.
    QTimer::singleShot(250,&app,[&] {
        publish();
        if (clipboard->mimeData(QClipboard::Clipboard)!=current) {event(QStringLiteral("error KDE background clipboard access is unavailable"));app.quit();}
        else event(QStringLiteral("ready"));
    });
    return app.exec();
}

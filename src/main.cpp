#include "window.h"
#include <QApplication>
#include <QCommandLineParser>
#include <QCoreApplication>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QTextStream>
#include <QTimer>
#include <sys/stat.h>
#include <unistd.h>
using namespace Sleeper;
static void output(QJsonObject obj) {
    QTextStream(stdout) << QJsonDocument(obj).toJson(QJsonDocument::Indented);
}
int main(int argc, char **argv) {
    bool gui = argc == 1;
    if (QFileInfo(QString::fromLocal8Bit(argv[0])).fileName() == "sleep-scheduler-control")
        gui = false;
    std::unique_ptr<QCoreApplication> app;
    if (gui)
        app = std::make_unique<QApplication>(argc, argv);
    else
        app = std::make_unique<QCoreApplication>(argc, argv);
    QCoreApplication::setApplicationName("RTC Sleeper");
    QCoreApplication::setOrganizationName("RTCSleeper");
    QCoreApplication::setApplicationVersion("2.0.0");
    try {
        if (gui) {
            Client client;
            Window w(client);
            w.show();
            return app->exec();
        }
        auto args = app->arguments();
        Host host;
        Engine engine("/var/lib/sleep-scheduler", host);
        if (QFileInfo(args[0]).fileName() == "sleep-scheduler-control") {
            if (geteuid() != 0 || args.size() != 2 || args[1].toUtf8().size() > 8192)
                fail("Invalid privileged helper invocation.");
            umask(0022);
            QJsonParseError e;
            auto doc = QJsonDocument::fromJson(args[1].toUtf8(), &e);
            if (e.error != QJsonParseError::NoError || !doc.isObject())
                fail("Invalid JSON request.");
            bool ok = false;
            auto uid = qEnvironmentVariable("SUDO_UID").toLongLong(&ok);
            if (!ok || uid <= 0)
                fail("Invoke helper through the authorized sudo rule.");
            output({{"ok", true}, {"state", engine.control(doc.object(), uid)}});
            return 0;
        }
        if (args.size() == 2 && (args[1] == "--daemon" || args[1] == "--resumed")) {
            if (geteuid() != 0)
                fail("Administrator privileges required.");
            umask(0022);
            if (args[1] == "--resumed") {
                engine.resumed();
                return 0;
            }
            engine.startup();
            QTimer timer;
            QObject::connect(&timer, &QTimer::timeout, app.get(), [&] {
                try {
                    engine.tick();
                } catch (const std::exception &e) {
                    QTextStream(stderr) << e.what() << Qt::endl;
                }
            });
            timer.start(1000);
            return app->exec();
        }
        QCommandLineParser p;
        p.setApplicationDescription(
            "Verified RTC sleep/wake schedules. Commands return JSON. No arguments opens the GUI.");
        p.addHelpOption();
        p.addVersionOption();
        p.addOption(QCommandLineOption(QStringLiteral("actor"), "user or agent", "kind", "user"));
        p.addOption(QCommandLineOption(QStringLiteral("agent-name"), "Self-declared agent name",
                                       "name", ""));
        p.addOption(
            QCommandLineOption(QStringLiteral("model"), "Self-declared model label", "model", ""));
        p.addOption(QCommandLineOption(QStringLiteral("sleep-at"), "Local 24-hour HH:MM", "HH:MM"));
        p.addOption(QCommandLineOption(QStringLiteral("wake-at"), "Local 24-hour HH:MM", "HH:MM"));
        p.addOption(QCommandLineOption(QStringLiteral("now"),
                                       "Sleep in 5 seconds after verified RTC alarm"));
        p.addOption(
            QCommandLineOption(QStringLiteral("repeat"), "daily or mon,tue,...", "days", ""));
        p.addOption(QCommandLineOption(QStringLiteral("replace-alarm"),
                                       "Explicit permission to replace an external wake alarm"));
        p.addOption(QCommandLineOption(QStringLiteral("limit"), "Number of log records (1–2000)",
                                       "count", "50"));
        p.addPositionalArgument("command", "status | logs | schedule | pause | resume | cancel");
        if (!p.parse(args))
            fail(p.errorText());
        if (p.isSet("help") || p.isSet("help-all")) {
            QTextStream(stdout) << p.helpText();
            return 0;
        }
        if (p.isSet("version")) {
            QTextStream(stdout) << "2.0.0\n";
            return 0;
        }
        if (p.positionalArguments().size() != 1)
            fail("Specify one command. Use --help for usage.");
        QString op = p.positionalArguments()[0];
        if (!QStringList{"status", "logs", "schedule", "pause", "resume", "cancel"}.contains(op))
            fail("Unsupported command.");
        for (auto name : {"sleep-at", "wake-at", "now", "repeat"})
            if (p.isSet(name) && op != "schedule")
                fail(QString("--%1 only applies to schedule.").arg(name));
        if (p.isSet("replace-alarm") && op != "schedule" && op != "resume")
            fail("--replace-alarm only applies to schedule/resume.");
        if (p.isSet("limit") && op != "logs")
            fail("--limit only applies to logs.");
        if (!QStringList{"user", "agent"}.contains(p.value("actor")))
            fail("Actor must be user or agent.");
        Client client;
        QJsonValue result;
        if (op == "status")
            result = client.status();
        else if (op == "logs") {
            bool ok;
            int n = p.value("limit").toInt(&ok);
            if (!ok)
                fail("Invalid log limit.");
            result = client.logs(n);
        } else {
            QJsonObject payload;
            auto actor = userActor();
            actor["kind"] = p.value("actor");
            actor["name"] = p.value("agent-name").isEmpty()
                                ? (p.value("actor") == "agent" ? "AI Agent" : "User")
                                : p.value("agent-name");
            actor["model"] = p.value("model");
            if (op == "schedule") {
                if (!p.isSet("wake-at") || p.isSet("now") == p.isSet("sleep-at"))
                    fail("Provide --wake-at and exactly one of --sleep-at / --now.");
                QJsonArray days;
                auto repeat = p.value("repeat");
                QStringList names{"mon", "tue", "wed", "thu", "fri", "sat", "sun"};
                if (repeat == "daily")
                    for (int i = 0; i < 7; i++)
                        days.append(i);
                else if (!repeat.isEmpty())
                    for (auto d : repeat.split(',')) {
                        int i = names.indexOf(d.trimmed().toLower());
                        if (i < 0)
                            fail("Invalid repeat weekday.");
                        days.append(i);
                    }
                QJsonObject c{{"sleep_time", p.isSet("now") ? "00:00" : p.value("sleep-at")},
                              {"wake_time", p.value("wake-at")},
                              {"now", p.isSet("now")},
                              {"days", days}};
                validateConfig(c);
                payload = {{"config", c}, {"replace_alarm", p.isSet("replace-alarm")}};
            } else if (op == "resume")
                payload = {{"replace_alarm", p.isSet("replace-alarm")}};
            result = client.request(op, payload, actor);
        }
        output({{"ok", true}, {"result", result}});
        return 0;
    } catch (const std::exception &e) {
        output({{"ok", false}, {"error", QString::fromUtf8(e.what())}});
        return 1;
    }
}

#include "client.h"
#include <QFile>
#include <QJsonDocument>
#include <QProcess>
namespace Sleeper {
QJsonObject userActor(QString source) {
    return {{"kind", "user"}, {"name", "User"}, {"model", ""}, {"source", source}};
}
QJsonObject Client::status() {
    Host host;
    Engine e("/var/lib/sleep-scheduler", host);
    auto s = e.state();
    try {
        s["actual_alarm"] = jsonAlarm(host.alarm());
        s["rtc_readable"] = true;
    } catch (...) {
        s["rtc_readable"] = false;
    }
    QProcess p;
    p.start("/usr/bin/systemctl", {"is-active", "sleep-scheduler.service"});
    p.waitForFinished(2500);
    bool running = p.readAllStandardOutput().trimmed() == "active";
    s["engine_running"] = running;
    if (s["status"] == "scheduled") {
        if (!running) {
            s["status"] = "error";
            s["message"] = "Scheduling engine is not running.";
        } else if (!s["rtc_readable"].toBool() || s["actual_alarm"] != s["expected_alarm"]) {
            s["status"] = "conflict";
            s["message"] = "RTC alarm differs from schedule; sleep is blocked.";
        }
    }
    return s;
}
QJsonObject Client::request(QString op, QJsonObject payload, QJsonObject actor) {
    if (actor.isEmpty())
        actor = userActor();
    auto envelope = QJsonDocument(QJsonObject{{"op", op}, {"payload", payload}, {"actor", actor}})
                        .toJson(QJsonDocument::Compact);
    QProcess p;
    p.start("/usr/bin/sudo",
            {"-n", "/usr/local/libexec/sleep-scheduler-control", QString::fromUtf8(envelope)});
    if (!p.waitForStarted(5000))
        fail("Cannot start privileged helper.");
    if (!p.waitForFinished(45000)) {
        p.kill();
        p.waitForFinished();
        fail("Helper timed out; refresh status before retrying.");
    }
    auto bytes = p.readAllStandardOutput();
    auto reply = QJsonDocument::fromJson(bytes).object();
    if (p.exitCode() != 0 || !reply["ok"].toBool())
        fail(reply["error"].toString(QString::fromUtf8(p.readAllStandardError()).trimmed() +
                                     " Install the engine to enable scheduling."));
    return reply["state"].toObject();
}
QJsonArray Client::logs(int limit) {
    if (limit < 1 || limit > 2000)
        fail("Log limit must be 1–2000.");
    QList<QJsonObject> records;
    for (auto name : {"events.previous.jsonl", "events.jsonl"}) {
        QFile f(QString("/var/lib/sleep-scheduler/") + name);
        if (!f.exists())
            continue;
        if (!f.open(QIODevice::ReadOnly))
            fail("Cannot read audit log.");
        while (!f.atEnd()) {
            auto d = QJsonDocument::fromJson(f.readLine());
            if (d.isObject())
                records.append(d.object());
        }
    }
    QJsonArray out;
    for (int i = records.size() - 1; i >= 0 && out.size() < limit; --i)
        out.append(records[i]);
    return out;
}
} // namespace Sleeper

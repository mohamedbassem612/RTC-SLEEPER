#include "engine.h"
#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QProcess>
#include <QRegularExpression>
#include <QSaveFile>
#include <QUuid>
#include <cmath>
#include <fcntl.h>
#include <sys/file.h>
#include <unistd.h>

namespace Sleeper {
namespace {
QJsonObject systemActor() {
    return {{"kind", "system"}, {"name", "Scheduler"}, {"source", "system"}, {"model", ""}};
}
class Lock {
    int fd_;

  public:
    explicit Lock(const QString &root) {
        if (!QDir().mkpath(root))
            fail("Cannot create state directory.");
        fd_ = ::open(QFile::encodeName(root + "/lock").constData(),
                     O_CREAT | O_RDWR | O_CLOEXEC | O_NOFOLLOW, 0644);
        if (fd_ < 0)
            fail("Cannot lock scheduler state.");
        if (flock(fd_, LOCK_EX) < 0) {
            ::close(fd_);
            fail("Cannot lock scheduler state.");
        }
    }
    ~Lock() {
        if (fd_ >= 0) {
            flock(fd_, LOCK_UN);
            ::close(fd_);
        }
    }
};
bool keys(const QJsonObject &o, QStringList k) {
    auto a = o.keys();
    a.sort();
    k.sort();
    return a == k;
}
void timeValid(const QJsonValue &v) {
    static QRegularExpression re("\\A(?:[01][0-9]|2[0-3]):[0-5][0-9]\\z");
    if (!v.isString() || !re.match(v.toString()).hasMatch())
        fail("Time must be HH:MM in 24-hour format (00:00–23:59).");
}
qint64 localStamp(QDate d, QString time) {
    auto t = QTime::fromString(time, "HH:mm");
    QDateTime stamp(d, t, Qt::LocalTime);
    if (!stamp.isValid() || stamp.date() != d || stamp.time() != t)
        return -1;
    return stamp.toSecsSinceEpoch();
}
} // namespace
QJsonValue jsonAlarm(Alarm a) {
    return a ? QJsonValue(*a) : QJsonValue(QJsonValue::Null);
}
Alarm alarmValue(QJsonValue v) {
    return v.isDouble() ? Alarm(v.toInteger()) : std::nullopt;
}
QJsonObject idleState() {
    QJsonObject s{{"version", 2},
                  {"status", "idle"},
                  {"message", ""},
                  {"updated", QDateTime::currentSecsSinceEpoch()}};
    for (auto k : {"config", "next_sleep", "next_wake", "expected_alarm", "actual_alarm", "actor",
                   "cycle", "last_resume", "trigger", "scheduled_at"})
        s[k] = QJsonValue::Null;
    return s;
}
QString run(const QString &p, const QStringList &args, int timeout) {
    QProcess proc;
    proc.setProcessEnvironment(QProcessEnvironment::systemEnvironment());
    proc.start(p, args);
    if (!proc.waitForStarted(5000))
        fail("Cannot start " + p);
    if (!proc.waitForFinished(timeout)) {
        proc.kill();
        proc.waitForFinished();
        fail("Command timed out: " + p);
    }
    if (proc.exitStatus() != QProcess::NormalExit || proc.exitCode() != 0)
        fail(QString::fromUtf8(proc.readAllStandardError()).trimmed() + " " +
             QString::fromUtf8(proc.readAllStandardOutput()).trimmed());
    return QString::fromUtf8(proc.readAllStandardOutput()).trimmed();
}
qint64 Host::now() const {
    return QDateTime::currentSecsSinceEpoch();
}
Alarm Host::alarm() const {
    QFile f("/sys/class/rtc/rtc0/wakealarm");
    if (!f.open(QIODevice::ReadOnly))
        fail("Cannot read RTC wake alarm.");
    const auto value = f.readAll().trimmed();
    if (value.isEmpty() || value == "0")
        return {};
    bool ok = false;
    auto n = value.toLongLong(&ok);
    if (!ok || n < 0)
        fail("Invalid RTC wake alarm.");
    return n;
}
void Host::arm(qint64 stamp) {
    run("/usr/sbin/rtcwake", {"-m", "no", "-t", QString::number(stamp)});
}
void Host::clear() {
    run("/usr/sbin/rtcwake", {"-m", "disable"});
}
void Host::suspend() {
    run("/usr/bin/systemctl", {"--no-block", "suspend"});
}
void validateConfig(const QJsonObject &c) {
    if (!keys(c, {"sleep_time", "wake_time", "now", "days"}))
        fail("Invalid schedule fields.");
    timeValid(c["sleep_time"]);
    timeValid(c["wake_time"]);
    if (!c["now"].isBool() || !c["days"].isArray())
        fail("Invalid schedule types.");
    QSet<int> seen;
    for (auto v : c["days"].toArray()) {
        double d = v.toDouble(-1);
        if (!v.isDouble() || !std::isfinite(d) || d < 0 || d > 6 || std::floor(d) != d)
            fail("Invalid repeat weekdays.");
        int n = static_cast<int>(d);
        if (seen.contains(n))
            fail("Invalid repeat weekdays.");
        seen.insert(n);
    }
    if (c["now"].toBool() && !seen.isEmpty())
        fail("Sleep Now cannot repeat.");
    if (!c["now"].toBool() && c["sleep_time"] == c["wake_time"])
        fail("Sleep and wake times must differ.");
}
std::pair<qint64, qint64> nextCycle(const QJsonObject &c, qint64 now, QDate after) {
    validateConfig(c);
    auto day = QDateTime::fromSecsSinceEpoch(now).date();
    qint64 sleep = -1;
    if (c["now"].toBool())
        sleep = now + 5;
    else
        for (int i = 0; i < 16; i++) {
            auto d = day.addDays(i);
            auto days = c["days"].toArray();
            if (after.isValid() && d <= after)
                continue;
            if (!days.isEmpty() && !days.contains(d.dayOfWeek() - 1))
                continue;
            auto stamp = localStamp(d, c["sleep_time"].toString());
            if (stamp > now + 2) {
                sleep = stamp;
                break;
            }
        }
    if (sleep < 0)
        fail("Could not find the next valid sleep date.");
    auto sleepDay = QDateTime::fromSecsSinceEpoch(sleep).date();
    for (int i = 0; i < 4; i++) {
        auto wake = localStamp(sleepDay.addDays(i), c["wake_time"].toString());
        if (wake > sleep + 15)
            return {sleep, wake};
    }
    fail("Could not find a valid wake date.");
}
Engine::Engine(QString root, Host &host) : root_(std::move(root)), host_(host) {}
QJsonObject Engine::state() const {
    QFile f(root_ + "/state.json");
    if (!f.exists())
        return idleState();
    if (!f.open(QIODevice::ReadOnly))
        fail("Cannot read scheduler state.");
    QJsonParseError e;
    auto d = QJsonDocument::fromJson(f.readAll(), &e);
    if (e.error != QJsonParseError::NoError || !d.isObject())
        fail("Corrupt scheduler state.");
    return d.object();
}
void Engine::save(QJsonObject s) {
    s["updated"] = host_.now();
    QSaveFile f(root_ + "/state.json");
    if (!f.open(QIODevice::WriteOnly))
        fail("Cannot save state.");
    f.setPermissions(QFile::ReadOwner | QFile::WriteOwner | QFile::ReadGroup | QFile::ReadOther);
    auto bytes = QJsonDocument(s).toJson();
    if (f.write(bytes) != bytes.size() || !f.commit())
        fail("Cannot commit state.");
}
void Engine::event(const QString &action, QJsonObject details) {
    QFile f(root_ + "/events.jsonl");
    if (f.size() > 2000000) {
        QFile::remove(root_ + "/events.previous.jsonl");
        if (!f.rename(root_ + "/events.previous.jsonl"))
            fail("Cannot rotate logs.");
        f.setFileName(root_ + "/events.jsonl");
    }
    details["action"] = action;
    details["time"] = QDateTime::fromSecsSinceEpoch(host_.now()).toString(Qt::ISODate);
    if (!details.contains("actor"))
        details["actor"] = systemActor();
    if (!details.contains("result"))
        details["result"] = "ok";
    if (!f.open(QIODevice::WriteOnly | QIODevice::Append))
        fail("Cannot write audit log.");
    f.setPermissions(QFile::ReadOwner | QFile::WriteOwner | QFile::ReadGroup | QFile::ReadOther);
    f.write(QJsonDocument(details).toJson(QJsonDocument::Compact) + "\n");
}
void Engine::clearOwned(QJsonObject &s) {
    auto current = host_.alarm();
    auto expected = alarmValue(s["expected_alarm"]);
    if (expected && expected == current) {
        host_.clear();
        if (host_.alarm())
            fail("RTC alarm did not clear.");
    }
    s["expected_alarm"] = QJsonValue::Null;
    s["actual_alarm"] = jsonAlarm(host_.alarm());
}
void Engine::arm(QJsonObject &s, qint64 stamp, bool replace) {
    auto current = host_.alarm();
    if (current && current != alarmValue(s["expected_alarm"]) && !replace)
        fail("RTC_CONFLICT: another program owns the alarm. Explicit replacement required.");
    host_.arm(stamp);
    if (host_.alarm() != Alarm(stamp))
        fail("RTC verification failed.");
    s["expected_alarm"] = stamp;
    s["actual_alarm"] = stamp;
}
QJsonObject Engine::control(const QJsonObject &r, qint64 uid) {
    if (!keys(r, {"op", "payload", "actor"}) || !r["op"].isString() || !r["payload"].isObject() ||
        !r["actor"].isObject())
        fail("Invalid request.");
    auto actor = r["actor"].toObject();
    if (!keys(actor, {"kind", "name", "model", "source"}) ||
        !QStringList{"user", "agent"}.contains(actor["kind"].toString()) ||
        !QStringList{"gui", "cli"}.contains(actor["source"].toString()))
        fail("Invalid actor.");
    for (auto k : {"name", "model"}) {
        auto v = actor[k];
        if (!v.isString() || v.toString().size() > 100)
            fail("Invalid actor label.");
        for (auto ch : v.toString())
            if (ch.category() == QChar::Other_Control)
                fail("Invalid actor label.");
    }
    actor["uid"] = uid;
    auto op = r["op"].toString();
    auto p = r["payload"].toObject();
    Lock lock(root_);
    auto s = state();
    try {
        if (s["status"] == "sleeping")
            fail("Suspend is already in progress; wait for resume.");
        if (op == "schedule") {
            if (!keys(p, {"config", "replace_alarm"}) || !p["config"].isObject() ||
                !p["replace_alarm"].isBool())
                fail("Invalid schedule request.");
            auto c = p["config"].toObject();
            auto cycle = nextCycle(c, host_.now());
            auto fresh = s;
            arm(fresh, cycle.second, p["replace_alarm"].toBool());
            fresh["status"] = "scheduled";
            fresh["config"] = c;
            fresh["next_sleep"] = cycle.first;
            fresh["next_wake"] = cycle.second;
            fresh["actor"] = actor;
            fresh["cycle"] = QUuid::createUuid().toString(QUuid::WithoutBraces);
            fresh["trigger"] = QJsonValue::Null;
            fresh["message"] = "";
            fresh["scheduled_at"] = host_.now();
            save(fresh);
            event("schedule_created", {{"actor", actor},
                                       {"config", c},
                                       {"next_sleep", cycle.first},
                                       {"next_wake", cycle.second}});
            s = fresh;
        } else if (op == "pause") {
            if (!p.isEmpty() ||
                !QStringList{"scheduled", "conflict", "error"}.contains(s["status"].toString()))
                fail("No schedule to pause.");
            clearOwned(s);
            s["status"] = "paused";
            s["message"] = "Sleep and the owned wake alarm are paused.";
            save(s);
            event("schedule_paused", {{"actor", actor}});
        } else if (op == "resume") {
            if (!keys(p, {"replace_alarm"}) || !p["replace_alarm"].isBool() ||
                s["status"] != "paused" || !s["config"].isObject())
                fail("No paused schedule to resume.");
            auto cycle = nextCycle(s["config"].toObject(), host_.now());
            arm(s, cycle.second, p["replace_alarm"].toBool());
            s["status"] = "scheduled";
            s["next_sleep"] = cycle.first;
            s["next_wake"] = cycle.second;
            s["scheduled_at"] = host_.now();
            s["message"] = "";
            s["actor"] = actor;
            save(s);
            event("schedule_resumed", {{"actor", actor}});
        } else if (op == "cancel") {
            if (!p.isEmpty())
                fail("Cancel takes no payload.");
            clearOwned(s);
            auto fresh = idleState();
            fresh["actual_alarm"] = s["actual_alarm"];
            fresh["last_resume"] = s["last_resume"];
            save(fresh);
            s = fresh;
            event("schedule_removed", {{"actor", actor}});
        } else
            fail("Unsupported operation.");
    } catch (const std::exception &e) {
        event("request_failed",
              {{"actor", actor}, {"operation", op}, {"result", "error"}, {"error", e.what()}});
        throw;
    }
    return s;
}
void Engine::finish(QJsonObject &s, const QString &reason) {
    auto previous = s["next_sleep"].isDouble()
                        ? QDateTime::fromSecsSinceEpoch(s["next_sleep"].toInteger()).date()
                        : QDate();
    auto c = s["config"].toObject();
    clearOwned(s);
    s["status"] = "idle";
    s["next_sleep"] = QJsonValue::Null;
    s["next_wake"] = QJsonValue::Null;
    s["cycle"] = QJsonValue::Null;
    s["last_resume"] = host_.now();
    s["message"] = "Previous cycle completed and reset.";
    event("cycle_reset", {{"reason", reason}, {"trigger", s["trigger"]}});
    if (!c["days"].toArray().isEmpty()) {
        try {
            auto cycle = nextCycle(c, host_.now(), previous);
            arm(s, cycle.second);
            s["next_sleep"] = cycle.first;
            s["next_wake"] = cycle.second;
            s["status"] = "scheduled";
            s["scheduled_at"] = host_.now();
            s["cycle"] = QUuid::createUuid().toString();
            event("repeat_scheduled", {{"created_by", s["actor"]},
                                       {"next_sleep", cycle.first},
                                       {"next_wake", cycle.second}});
        } catch (const std::exception &e) {
            s["status"] = QString(e.what()).contains("RTC_CONFLICT") ? "conflict" : "error";
            s["message"] = e.what();
            event("repeat_failed", {{"result", "blocked"}, {"error", e.what()}});
        }
    } else {
        s["config"] = QJsonValue::Null;
        s["actor"] = QJsonValue::Null;
    }
    save(s);
}
void Engine::tick() {
    Lock lock(root_);
    auto s = state();
    try {
        auto actual = host_.alarm();
        bool changed = s["actual_alarm"] != jsonAlarm(actual);
        s["actual_alarm"] = jsonAlarm(actual);
        if (s["status"] == "scheduled") {
            if (actual != alarmValue(s["expected_alarm"])) {
                s["status"] = "conflict";
                s["message"] = "RTC alarm changed externally. Scheduled sleep blocked.";
                save(s);
                event("rtc_conflict", {{"result", "blocked"}, {"actual_alarm", jsonAlarm(actual)}});
                return;
            }
            auto now = host_.now();
            auto deadline = s["next_sleep"].toInteger();
            if (now >= deadline) {
                if (now - deadline > 60 || now >= s["next_wake"].toInteger() - 15) {
                    finish(s, "missed_sleep");
                    return;
                }
                if (host_.alarm() != alarmValue(s["next_wake"]))
                    fail("RTC changed immediately before suspend.");
                s["status"] = "sleeping";
                s["trigger"] = QJsonObject{
                    {"created_by", s["actor"]}, {"executed_by", systemActor()}, {"at", now}};
                save(s);
                event("sleep_triggered",
                      {{"created_by", s["actor"]}, {"next_wake", s["next_wake"]}});
                try {
                    host_.suspend();
                } catch (...) {
                    clearOwned(s);
                    throw;
                }
                return;
            }
        }
        if (changed)
            save(s);
    } catch (const std::exception &e) {
        s["status"] = "error";
        s["message"] = e.what();
        save(s);
        event("engine_error", {{"result", "error"}, {"error", e.what()}});
    }
}
void Engine::resumed() {
    Lock lock(root_);
    auto s = state();
    if (QStringList{"scheduled", "sleeping"}.contains(s["status"].toString()))
        finish(s, "system_resumed");
    else {
        s["last_resume"] = host_.now();
        save(s);
        event("system_resumed");
    }
}
void Engine::startup() {
    Lock lock(root_);
    auto s = state();
    if (s["status"] == "sleeping")
        finish(s, "engine_restarted");
    else
        save(s);
    event("engine_started");
}
} // namespace Sleeper

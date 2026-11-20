#include "engine.h"
#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QTemporaryDir>
#include <QtTest>
using namespace Sleeper;
class FakeHost : public Host {
  public:
    qint64 clock = QDateTime(QDate(2026, 11, 15), QTime(9, 0), Qt::LocalTime).toSecsSinceEpoch();
    Alarm rtc;
    int sleeps = 0, arms = 0;
    bool broken = false, failSleep = false;
    qint64 now() const override {
        return clock;
    }
    Alarm alarm() const override {
        return rtc;
    }
    void arm(qint64 stamp) override {
        ++arms;
        if (!broken)
            rtc = stamp;
    }
    void clear() override {
        rtc.reset();
    }
    void suspend() override {
        if (failSleep)
            fail("Inhibited");
        ++sleeps;
    }
};
static QJsonObject cfg() {
    return {
        {"sleep_time", "23:00"}, {"wake_time", "07:00"}, {"now", false}, {"days", QJsonArray{}}};
}
static QJsonObject request(QString op, QJsonObject payload = {}) {
    return {{"op", op},
            {"payload", payload},
            {"actor",
             QJsonObject{
                 {"kind", "agent"}, {"name", "Test"}, {"source", "cli"}, {"model", "test-model"}}}};
}
static QJsonObject schedule(QJsonObject c = cfg(), bool replace = false) {
    return request("schedule", {{"config", c}, {"replace_alarm", replace}});
}
class Tests : public QObject {
    Q_OBJECT
  private slots:
    void invalidTimes() {
        for (auto value : {"73:00", "24:00", "13:99", "7:00", "08:00\n", "$(reboot)"}) {
            auto c = cfg();
            c["wake_time"] = value;
            QVERIFY_EXCEPTION_THROWN(validateConfig(c), std::runtime_error);
        }
    }
    void invalidDays() {
        for (auto days : {QJsonArray{true}, QJsonArray{7}, QJsonArray{1, 1}, QJsonArray{1.5},
                          QJsonArray{1e300}, QJsonArray{"1"}}) {
            auto c = cfg();
            c["days"] = days;
            QVERIFY_EXCEPTION_THROWN(validateConfig(c), std::runtime_error);
        }
    }
    void overnight() {
        FakeHost h;
        auto cycle = nextCycle(cfg(), h.now());
        QCOMPARE(QDateTime::fromSecsSinceEpoch(cycle.first).date(), QDate(2026, 11, 15));
        QCOMPARE(QDateTime::fromSecsSinceEpoch(cycle.second).date(), QDate(2026, 11, 16));
    }
    void weekdays() {
        FakeHost h;
        auto c = cfg();
        c["days"] = QJsonArray{0};
        auto cycle = nextCycle(c, h.now());
        QCOMPARE(QDateTime::fromSecsSinceEpoch(cycle.first).date(), QDate(2026, 11, 16));
    }
    void dst() {
        auto old = qgetenv("TZ");
        qputenv("TZ", "America/New_York");
        tzset();
        auto c = cfg();
        c["sleep_time"] = "02:30";
        auto now = QDateTime(QDate(2026, 3, 8), QTime(0, 0), Qt::LocalTime).toSecsSinceEpoch();
        auto cycle = nextCycle(c, now);
        QCOMPARE(QDateTime::fromSecsSinceEpoch(cycle.first).date(), QDate(2026, 3, 9));
        if (old.isNull())
            qunsetenv("TZ");
        else
            qputenv("TZ", old);
        tzset();
    }
    void armVerified() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        auto s = e.control(schedule(), 1000);
        QCOMPARE(s["status"].toString(), "scheduled");
        QVERIFY(h.rtc == alarmValue(s["next_wake"]));
        QCOMPARE(h.sleeps, 0);
    }
    void armFailure() {
        QTemporaryDir dir;
        FakeHost h;
        h.broken = true;
        Engine e(dir.path(), h);
        QVERIFY_EXCEPTION_THROWN(e.control(schedule(), 1000), std::runtime_error);
        QCOMPARE(e.state()["status"].toString(), "idle");
        QCOMPARE(h.sleeps, 0);
    }
    void externalAlarm() {
        QTemporaryDir dir;
        FakeHost h;
        h.rtc = 9999999999LL;
        Engine e(dir.path(), h);
        QVERIFY_EXCEPTION_THROWN(e.control(schedule(), 1000), std::runtime_error);
        QCOMPARE(h.arms, 0);
        QVERIFY(h.rtc == Alarm(9999999999LL));
    }
    void explicitReplace() {
        QTemporaryDir dir;
        FakeHost h;
        h.rtc = 9999999999LL;
        Engine e(dir.path(), h);
        auto s = e.control(schedule(cfg(), true), 1000);
        QVERIFY(h.rtc == alarmValue(s["next_wake"]));
    }
    void overrideBlocks() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        auto s = e.control(schedule(), 1000);
        h.clock = s["next_sleep"].toInteger();
        h.rtc = 9999999999LL;
        e.tick();
        QCOMPARE(e.state()["status"].toString(), "conflict");
        QCOMPARE(h.sleeps, 0);
        e.control(request("cancel"), 1000);
        QVERIFY(h.rtc == Alarm(9999999999LL));
    }
    void pauseResume() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        e.control(schedule(), 1000);
        e.control(request("pause"), 1000);
        QVERIFY(!h.rtc);
        QCOMPARE(e.state()["status"].toString(), "paused");
        e.control(request("resume", {{"replace_alarm", false}}), 1000);
        QCOMPARE(e.state()["status"].toString(), "scheduled");
        QVERIFY(h.rtc);
    }
    void earlyWakeReset() {
        // Woken manually after the scheduled sleep: the cycle is complete and resets.
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        auto s = e.control(schedule(), 1000);
        h.clock = s["next_sleep"].toInteger();
        e.tick();
        QCOMPARE(h.sleeps, 1);
        e.resumed();
        QCOMPARE(e.state()["status"].toString(), "idle");
        QVERIFY(e.state()["config"].isNull());
        QVERIFY(!h.rtc);
    }
    void manualSuspendBeforeDeadlineKeepsSchedule() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        auto s = e.control(schedule(), 1000);
        h.clock += 3600; // unrelated suspend/resume hours before the sleep time
        e.resumed();
        QCOMPARE(e.state()["status"].toString(), "scheduled");
        QCOMPARE(e.state()["next_sleep"].toInteger(), s["next_sleep"].toInteger());
        QVERIFY(h.rtc);
        QVERIFY(!e.state()["last_resume"].isNull());
    }
    void resumeAfterDeadlineWithoutTickResets() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        auto s = e.control(schedule(), 1000);
        h.clock = s["next_sleep"].toInteger() + 10;
        e.resumed();
        QCOMPARE(e.state()["status"].toString(), "idle");
    }
    void sleepingWatchdog() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        auto s = e.control(schedule(), 1000);
        h.clock = s["next_sleep"].toInteger();
        e.tick();
        QCOMPARE(e.state()["status"].toString(), "sleeping");
        h.clock += 60;
        e.tick();
        QCOMPARE(e.state()["status"].toString(), "sleeping");
        h.clock += 120; // the resume hook never ran
        e.tick();
        QCOMPARE(e.state()["status"].toString(), "idle");
        e.control(schedule(), 1000); // usable again
    }
    void corruptStateRecovers() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        e.control(schedule(), 1000);
        QFile f(dir.path() + "/state.json");
        QVERIFY(f.open(QIODevice::WriteOnly | QIODevice::Truncate));
        f.write("{not json");
        f.close();
        e.tick();
        QCOMPARE(e.state()["status"].toString(), "idle");
        QVERIFY(!QDir(dir.path()).entryList({"state.corrupt.*"}).isEmpty());
        // The old alarm can no longer be proven ours, so replacing it needs explicit consent.
        e.control(schedule(cfg(), true), 1000);
        QCOMPARE(e.state()["status"].toString(), "scheduled");
    }
    void cycleIdFormatConsistent() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        auto c = cfg();
        c["days"] = QJsonArray{0, 1, 2, 3, 4, 5, 6};
        auto s = e.control(schedule(c), 1000);
        h.clock = s["next_sleep"].toInteger();
        e.tick();
        e.resumed();
        QVERIFY(!e.state()["cycle"].toString().contains('{'));
    }
    void wakeInsideDstGapDoesNotSlipADay() {
        auto old = qgetenv("TZ");
        qputenv("TZ", "America/New_York");
        tzset();
        auto c = cfg();
        c["wake_time"] = "02:30"; // does not exist on 2026-03-08
        auto now = QDateTime(QDate(2026, 3, 7), QTime(9, 0), Qt::LocalTime).toSecsSinceEpoch();
        auto cycle = nextCycle(c, now);
        QCOMPARE(QDateTime::fromSecsSinceEpoch(cycle.second).date(), QDate(2026, 3, 8));
        QVERIFY(cycle.second - cycle.first < 6 * 3600);
        if (old.isNull())
            qunsetenv("TZ");
        else
            qputenv("TZ", old);
        tzset();
    }
    void repeatNextDay() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        auto c = cfg();
        c["days"] = QJsonArray{0, 1, 2, 3, 4, 5, 6};
        auto s = e.control(schedule(c), 1000);
        h.clock = s["next_sleep"].toInteger();
        e.tick();
        QCOMPARE(h.sleeps, 1);
        e.resumed();
        QVERIFY(e.state()["next_sleep"].toInteger() > s["next_sleep"].toInteger());
        QCOMPARE(e.state()["status"].toString(), "scheduled");
        QCOMPARE(h.sleeps, 1);
    }
    void missedDeadline() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        auto s = e.control(schedule(), 1000);
        h.clock = s["next_sleep"].toInteger() + 180;
        e.tick();
        QCOMPARE(e.state()["status"].toString(), "idle");
        QCOMPARE(h.sleeps, 0);
    }
    void suspendFailure() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        auto s = e.control(schedule(), 1000);
        h.clock = s["next_sleep"].toInteger();
        h.failSleep = true;
        e.tick();
        QCOMPARE(e.state()["status"].toString(), "error");
        QVERIFY(!h.rtc);
    }
    void triggerAudit() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        auto s = e.control(schedule(), 1000);
        h.clock = s["next_sleep"].toInteger();
        e.tick();
        QCOMPARE(h.sleeps, 1);
        auto trigger = e.state()["trigger"].toObject();
        QCOMPARE(trigger["created_by"].toObject()["uid"].toInt(), 1000);
        QCOMPARE(trigger["executed_by"].toObject()["kind"].toString(), "system");
        QFile f(dir.path() + "/events.jsonl");
        QVERIFY(f.open(QIODevice::ReadOnly));
        QVERIFY(f.readAll().contains("test-model"));
        e.resumed();
        QCOMPARE(e.state()["status"].toString(), "idle");
    }
    void nowGrace() {
        FakeHost h;
        auto c = cfg();
        c["now"] = true;
        auto cycle = nextCycle(c, h.now());
        QCOMPARE(cycle.first, h.now() + 5);
        c["days"] = QJsonArray{0};
        QVERIFY_EXCEPTION_THROWN(validateConfig(c), std::runtime_error);
    }
    void rejectUnknown() {
        QTemporaryDir dir;
        FakeHost h;
        Engine e(dir.path(), h);
        QVERIFY_EXCEPTION_THROWN(e.control(request("poweroff"), 1000), std::runtime_error);
        QCOMPARE(h.sleeps, 0);
        QCOMPARE(h.arms, 0);
    }
};
QTEST_GUILESS_MAIN(Tests)
#include "test_engine.moc"

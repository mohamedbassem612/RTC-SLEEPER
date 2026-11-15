#include "window.h"
#include <QLineEdit>
#include <QTemporaryDir>
#include <QtTest>
using namespace Sleeper;
class FakeClient : public Client {
  public:
    QJsonObject s = idleState();
    int writes = 0;
    QJsonObject status() override {
        return s;
    }
    QJsonArray logs(int) override {
        return {};
    }
    QJsonObject request(QString op, QJsonObject, QJsonObject) override {
        ++writes;
        if (op == "cancel")
            s = idleState();
        return s;
    }
    void activate() {
        s = idleState();
        s["status"] = "scheduled";
        s["config"] = QJsonObject{{"sleep_time", "23:15"},
                                  {"wake_time", "07:45"},
                                  {"now", false},
                                  {"days", QJsonArray{}}};
        s["scheduled_at"] = QDateTime::currentSecsSinceEpoch();
        s["next_sleep"] = QDateTime::currentSecsSinceEpoch() + 3600;
        s["next_wake"] = QDateTime::currentSecsSinceEpoch() + 7200;
    }
};
class InterfaceTests : public QObject {
    Q_OBJECT
  private slots:
    void hourValidation() {
        TimeInput t("test");
        t.hour->setValue(7);
        t.hour->show();
        auto *line = t.hour->findChild<QLineEdit *>();
        line->selectAll();
        QTest::keyClicks(line, "73");
        QTest::keyClick(line, Qt::Key_Tab);
        QVERIFY(t.hour->value() >= 1 && t.hour->value() <= 12);
        QVERIFY(!line->text().contains("73"));
        t.setValue("23:59");
        QCOMPARE(t.value(), "23:59");
        t.setValue("00:00");
        QCOMPARE(t.hour->value(), 12);
        QCOMPARE(t.value(), "00:00");
    }
    void sleepNowDisablesWholeRow() {
        QTemporaryDir d;
        FakeClient c;
        Window w(c, d.filePath("prefs.ini"));
        auto *now = w.findChild<QCheckBox *>("sleepNow");
        auto *row = w.findChild<QWidget *>("sleepAtRow");
        now->setChecked(true);
        QVERIFY(!row->isEnabled());
        QVERIFY(!w.findChild<QSpinBox *>("sleepAtHour")->isEnabled());
        now->setChecked(false);
        QVERIFY(row->isEnabled());
        QCOMPARE(w.minimumSize(), w.maximumSize());
    }
    void removePreservesTimes() {
        QTemporaryDir d;
        FakeClient c;
        c.activate();
        {
            Window w(c, d.filePath("prefs.ini"));
            w.removeSchedule();
            QCOMPARE(c.writes, 1);
            QCOMPARE(w.findChild<QSpinBox *>("sleepAtHour")->value(), 11);
            QCOMPARE(w.findChild<QSpinBox *>("sleepAtMinute")->value(), 15);
            QCOMPARE(w.findChild<QSpinBox *>("wakeAtHour")->value(), 7);
            QCOMPARE(w.findChild<QSpinBox *>("wakeAtMinute")->value(), 45);
        }
        Window reopened(c, d.filePath("prefs.ini"));
        QCOMPARE(reopened.findChild<QSpinBox *>("sleepAtHour")->value(), 11);
        QCOMPARE(reopened.findChild<QSpinBox *>("wakeAtMinute")->value(), 45);
    }
    void cancelEditingLeavesSchedule() {
        QTemporaryDir d;
        FakeClient c;
        c.activate();
        Window w(c, d.filePath("prefs.ini"));
        auto original = c.s;
        w.beginEdit();
        w.findChild<QSpinBox *>("sleepAtHour")->setValue(4);
        w.findChild<QSpinBox *>("wakeAtMinute")->setValue(3);
        w.cancelEdit();
        QCOMPARE(c.writes, 0);
        QCOMPARE(c.s, original);
        w.beginEdit();
        QCOMPARE(w.findChild<QSpinBox *>("sleepAtHour")->value(), 11);
        QCOMPARE(w.findChild<QSpinBox *>("wakeAtMinute")->value(), 45);
    }
    void resumeResetsFormKeepsChoices() {
        QTemporaryDir d;
        FakeClient c;
        c.activate();
        Window w(c, d.filePath("prefs.ini"));
        w.refresh();
        c.s = idleState();
        w.refresh();
        QCOMPARE(w.findChild<QSpinBox *>("sleepAtMinute")->value(), 15);
        QCOMPARE(w.findChild<QSpinBox *>("wakeAtMinute")->value(), 45);
        QCOMPARE(c.writes, 0);
    }
    void renderPreviews() {
        QTemporaryDir d;
        FakeClient c;
        Window w(c, d.filePath("prefs.ini"));
        w.show();
        QTest::qWait(40);
        QVERIFY(w.grab().save("preview-idle.png"));
        w.findChild<QCheckBox *>("sleepNow")->setChecked(true);
        QTest::qWait(40);
        QVERIFY(w.grab().save("preview-now.png"));
        c.activate();
        w.refresh();
        QTest::qWait(350);
        QVERIFY(w.grab().save("preview-active.png"));
    }
    void arabicLayout() {
        QTemporaryDir d;
        auto file = d.filePath("prefs.ini");
        {
            QSettings s(file, QSettings::IniFormat);
            s.setValue("language", "ar");
        }
        FakeClient c;
        Window w(c, file);
        w.show();
        QTest::qWait(40);
        QCOMPARE(w.layoutDirection(), Qt::RightToLeft);
        QVERIFY(w.grab().save("preview-arabic.png"));
    }
};
QTEST_MAIN(InterfaceTests)
#include "test_interface.moc"

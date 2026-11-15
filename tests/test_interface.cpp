#include "window.h"
#include <QElapsedTimer>
#include <QLineEdit>
#include <QTemporaryDir>
#include <QToolButton>
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
        auto *now = w.findChild<ToggleSwitch *>("sleepNow");
        auto *row = w.findChild<QWidget *>("sleepAtRow");
        now->setChecked(true);
        QVERIFY(!row->isEnabled());
        QVERIFY(!w.findChild<QSpinBox *>("sleepAtHour")->isEnabled());
        QVERIFY(!w.findChild<ToggleSwitch *>("repeatDay0")->isEnabled());
        now->setChecked(false);
        QVERIFY(row->isEnabled());
        QCOMPARE(w.minimumSize(), w.maximumSize());
    }
    void togglesAnimateAndSupportKeyboard() {
        ToggleSwitch toggle;
        toggle.show();
        QTest::qWait(20);
        QCOMPARE(toggle.thumbPosition(), 0.0);
        QTest::mouseClick(&toggle, Qt::LeftButton);
        QVERIFY(toggle.isChecked());
        QTest::qWait(80);
        QVERIFY(toggle.thumbPosition() > 0.0 && toggle.thumbPosition() < 1.0);
        QTest::qWait(140);
        QCOMPARE(toggle.thumbPosition(), 1.0);
        QTest::keyClick(&toggle, Qt::Key_Space);
        QVERIFY(!toggle.isChecked());
        QTest::qWait(220);
        QCOMPARE(toggle.thumbPosition(), 0.0);
        QTest::mouseClick(&toggle, Qt::LeftButton);
        QTest::qWait(60);
        auto beforeReverse = toggle.thumbPosition();
        QTest::mouseClick(&toggle, Qt::LeftButton);
        QCOMPARE(toggle.thumbPosition(), beforeReverse);
        QTest::qWait(220);
        QCOMPARE(toggle.thumbPosition(), 0.0);
        toggle.setEnabled(false);
        QTest::mouseClick(&toggle, Qt::LeftButton);
        QVERIFY(!toggle.isChecked());
        toggle.hide();
        toggle.setChecked(true);
        QCOMPARE(toggle.thumbPosition(), 1.0);
    }
    void arrowsStepAndAnimate() {
        TimeInput t("test");
        t.setValue("07:20");
        t.show();
        QTest::qWait(20);
        auto *hourUp = t.findChild<QToolButton *>("testHourIncrease");
        auto *hourDown = t.findChild<QToolButton *>("testHourDecrease");
        auto *minuteUp = t.findChild<QToolButton *>("testMinuteIncrease");
        auto *minuteDown = t.findChild<QToolButton *>("testMinuteDecrease");
        QVERIFY(hourUp && hourDown && minuteUp && minuteDown);
        QTest::mouseClick(hourUp, Qt::LeftButton);
        QCOMPARE(t.hour->value(), 8);
        auto *animation = t.hour->findChild<QWidget *>("testHourDigitAnimation");
        QVERIFY(animation && animation->isVisible());
        QTest::qWait(160);
        QVERIFY(!animation->isVisible());
        QTest::mouseClick(hourDown, Qt::LeftButton);
        QCOMPARE(t.hour->value(), 7);
        QTest::mouseClick(minuteUp, Qt::LeftButton);
        QCOMPARE(t.minute->value(), 21);
        QTest::mouseClick(minuteDown, Qt::LeftButton);
        QCOMPARE(t.minute->value(), 20);
        t.hour->setValue(12);
        QTest::mouseClick(hourUp, Qt::LeftButton);
        QCOMPARE(t.hour->value(), 1);
        t.minute->setValue(0);
        QTest::mouseClick(minuteDown, Qt::LeftButton);
        QCOMPARE(t.minute->value(), 59);
    }
    void holdAcceleratesWithinLimitAndStops() {
        TimeInput t("test");
        t.setValue("07:10");
        t.show();
        QTest::qWait(20);
        auto *up = t.findChild<QToolButton *>("testMinuteIncrease");
        QVector<qint64> times;
        QElapsedTimer elapsed;
        elapsed.start();
        connect(t.minute, &QSpinBox::valueChanged, this, [&] { times.append(elapsed.elapsed()); });
        QTest::mousePress(up, Qt::LeftButton);
        QTest::qWait(4100);
        QTest::mouseRelease(up, Qt::LeftButton);
        QVERIFY(times.size() >= 12 && times.size() <= 30);
        QVERIFY(times[1] - times[0] >= 350); // initial hold delay
        for (int i = 2; i < times.size(); ++i)
            QVERIFY2(times[i] - times[i - 1] >= 125, "Hold exceeded the maximum repeat speed");
        QVERIFY(times[2] - times[1] > times.last() - times[times.size() - 2]);
        int stopped = t.minute->value();
        QTest::qWait(450);
        QCOMPARE(t.minute->value(), stopped);
        auto *down = t.findChild<QToolButton *>("testMinuteDecrease");
        QTest::mousePress(down, Qt::LeftButton);
        QTest::qWait(480);
        QTest::mouseRelease(down, Qt::LeftButton);
        QCOMPARE(t.minute->value(), stopped - 2);
        QTest::mousePress(up, Qt::LeftButton);
        t.setEnabled(false);
        stopped = t.minute->value();
        QTest::qWait(500);
        QCOMPARE(t.minute->value(), stopped);
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
        w.findChild<ToggleSwitch *>("sleepNow")->setChecked(true);
        QTest::qWait(220);
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

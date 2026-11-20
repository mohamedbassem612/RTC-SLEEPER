#pragma once
#include "client.h"
#include <QAbstractButton>
#include <QComboBox>
#include <QFrame>
#include <QLabel>
#include <QPushButton>
#include <QSettings>
#include <QSpinBox>
#include <QStackedWidget>
#include <QVariantAnimation>
#include <QWidget>
#include <functional>
#include <future>
namespace Sleeper {
class ToggleSwitch : public QAbstractButton {
    Q_OBJECT
    qreal position_ = 0;
    QVariantAnimation animation_;

  protected:
    void paintEvent(QPaintEvent *) override;
    void hideEvent(QHideEvent *) override;

  public:
    explicit ToggleSwitch(QWidget *parent = nullptr);
    QSize sizeHint() const override;
    qreal thumbPosition() const {
        return position_;
    }
};
class TimeInput : public QFrame {
  public:
    QSpinBox *hour, *minute;
    QComboBox *period;
    explicit TimeInput(QString name, QWidget *parent = nullptr);
    QString value() const;
    void setValue(QString hhmm);
    std::function<void()> changed;
};
class Window : public QWidget {
    Client &client_;
    QSettings settings_;
    bool arabic_ = false, editing_ = false, building_ = false, busy_ = false, dropPending_ = false;
    std::future<QJsonObject> pending_;
    qint64 lastStatus_ = 0;
    QJsonObject state_, last_, snapshot_;
    QStackedWidget *pages_{};
    QWidget *form_{}, *dashboard_{}, *sleepRow_{}, *repeatRow_{};
    TimeInput *sleep_{}, *wake_{};
    ToggleSwitch *now_{};
    QList<ToggleSwitch *> days_;
    QLabel *duration_{}, *countdown_{}, *summary_{}, *badge_{}, *ringText_{}, *next_{};
    QPushButton *cancelEdit_{}, *pause_{};
    QString text(QString en, QString ar) const;
    QWidget *makeForm();
    QWidget *makeDashboard();
    void build();
    void autoRefresh();
    void applyStatus(QJsonObject fresh);
    QJsonObject runBlocking(std::function<QJsonObject()> fn);
    void switchPage(QWidget *page, bool animate = true);
    void setConfig(QJsonObject c);
    QJsonObject config() const;
    void remember(QJsonObject c);
    void updateForm();
    void updateDashboard();
    void schedule();
    bool operation(QString op, QJsonObject p = {});
    void settingsDialog();
    void remoteDialog();
    void logsDialog();

  public:
    explicit Window(Client &client, QString settingsFile = {});
    void refresh();
    void beginEdit();
    void cancelEdit();
    void removeSchedule();
};
} // namespace Sleeper

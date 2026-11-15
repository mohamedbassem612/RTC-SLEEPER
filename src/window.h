#pragma once
#include "client.h"
#include <QCheckBox>
#include <QComboBox>
#include <QFrame>
#include <QLabel>
#include <QPushButton>
#include <QSettings>
#include <QSpinBox>
#include <QStackedWidget>
#include <QWidget>
#include <functional>
namespace Sleeper {
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
    bool arabic_ = false, editing_ = false, building_ = false;
    QJsonObject state_, last_, snapshot_;
    QStackedWidget *pages_{};
    QWidget *form_{}, *dashboard_{}, *sleepRow_{}, *repeatRow_{};
    TimeInput *sleep_{}, *wake_{};
    QCheckBox *now_{};
    QList<QCheckBox *> days_;
    QLabel *duration_{}, *countdown_{}, *summary_{}, *badge_{}, *ringText_{}, *next_{};
    QPushButton *cancelEdit_{}, *pause_{};
    QString text(QString en, QString ar) const;
    QWidget *makeForm();
    QWidget *makeDashboard();
    void build();
    void switchPage(QWidget *page, bool animate = true);
    void setConfig(QJsonObject c);
    QJsonObject config() const;
    void remember(QJsonObject c);
    void updateForm();
    void updateDashboard();
    void schedule();
    bool operation(QString op, QJsonObject p = {});
    void settingsDialog();
    void logsDialog();

  public:
    explicit Window(Client &client, QString settingsFile = {});
    void refresh();
    void beginEdit();
    void cancelEdit();
    void removeSchedule();
};
} // namespace Sleeper

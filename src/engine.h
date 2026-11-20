#pragma once
#include <QDateTime>
#include <QJsonObject>
#include <QString>
#include <optional>
#include <stdexcept>

namespace Sleeper {
[[noreturn]] inline void fail(const QString &s) {
    throw std::runtime_error(s.toStdString());
}
using Alarm = std::optional<qint64>;
QJsonValue jsonAlarm(Alarm a);
Alarm alarmValue(QJsonValue v);
QJsonObject idleState();
void validateConfig(const QJsonObject &c);
std::pair<qint64, qint64> nextCycle(const QJsonObject &c, qint64 now, QDate after = {});
class Host {
  public:
    virtual ~Host() = default;
    virtual qint64 now() const;
    virtual Alarm alarm() const;
    virtual void arm(qint64 stamp);
    virtual void clear();
    virtual void suspend();
};
QString run(const QString &program, const QStringList &args, int timeout = 35000);
class Engine {
    QString root_;
    Host &host_;
    QJsonObject stateOrRecover();
    void save(QJsonObject s);
    void clearOwned(QJsonObject &s);
    void arm(QJsonObject &s, qint64 stamp, bool replace = false);
    void finish(QJsonObject &s, const QString &reason);

  public:
    Engine(QString root, Host &host);
    QJsonObject state() const;
    QJsonObject control(const QJsonObject &request, qint64 uid);
    void tick();
    void resumed();
    void sleeping();
    void startup();
    void event(const QString &action, QJsonObject details = {});
};
} // namespace Sleeper

#pragma once
#include "engine.h"
#include <QJsonArray>
namespace Sleeper {
class Client {
  public:
    virtual ~Client() = default;
    virtual QJsonObject status();
    virtual QJsonObject request(QString op, QJsonObject payload = {}, QJsonObject actor = {});
    virtual QJsonArray logs(int limit = 100);
};
QJsonObject userActor(QString source = "cli");
} // namespace Sleeper

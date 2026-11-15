#include "window.h"
#include <QApplication>
#include <QDateTime>
#include <QDialog>
#include <QDialogButtonBox>
#include <QGraphicsOpacityEffect>
#include <QHBoxLayout>
#include <QJsonDocument>
#include <QLocale>
#include <QMessageBox>
#include <QPainter>
#include <QPropertyAnimation>
#include <QScrollArea>
#include <QStandardPaths>
#include <QTextEdit>
#include <QTimer>
#include <QVBoxLayout>

namespace Sleeper {
namespace {
class TwoDigitSpin : public QSpinBox {
  protected:
    QString textFromValue(int value) const override {
        return QString::number(value).rightJustified(2, '0');
    }
};
QLabel *label(QString s, QString role = "", QWidget *parent = nullptr) {
    auto *l = new QLabel(s, parent);
    l->setProperty("role", role);
    return l;
}
QPushButton *button(QString s, QString name = "") {
    auto *b = new QPushButton(s);
    b->setObjectName(name);
    b->setCursor(Qt::PointingHandCursor);
    b->setMinimumHeight(48);
    return b;
}
QString remaining(qint64 seconds, bool ar = false, bool showSeconds = true) {
    seconds = qMax(qint64(0), seconds);
    auto out = QString(ar ? "%1 س %2 د" : "%1 h %2 min").arg(seconds / 3600).arg(seconds / 60 % 60);
    return showSeconds ? out + QString(ar ? " %1 ث" : " %1 s").arg(seconds % 60) : out;
}
QString timeLabel(QJsonValue v) {
    return v.isDouble() ? QLocale(QLocale::English)
                              .toString(QDateTime::fromSecsSinceEpoch(v.toInteger()),
                                        "ddd, dd MMM · hh:mm AP")
                        : "—";
}
class Ring : public QFrame {
  public:
    double progress = 0;
    explicit Ring(QWidget *p = nullptr) : QFrame(p) {
        setFixedSize(370, 370);
    }
    void paintEvent(QPaintEvent *) override {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        QRectF r(12, 12, width() - 24, height() - 24);
        p.setPen(QPen(QColor("#242424"), 10));
        p.drawEllipse(r);
        p.setPen(QPen(Qt::white, 10, Qt::SolidLine, Qt::RoundCap));
        p.drawArc(r, 90 * 16, -int(qBound(0.02, progress, 1.0) * 360 * 16));
    }
};
const char *appStyle = R"(
QWidget { background: #000; color: #f5f5f5; font-family: 'Noto Sans', 'DejaVu Sans'; font-size: 15px; }
QFrame[role="card"] { background: #0c0c0c; border: 1px solid #292929; border-radius: 18px; }
QLabel { background: transparent; border: none; color: #f5f5f5; }
QLabel[role="title"] {font-size: 30px; font-weight: 700;}
QLabel[role="heading"] {font-size: 21px; font-weight: 600;}
QLabel[role="muted"] {color: #999; font-size: 13px;}
QLabel[role="duration"] {font-size: 27px; font-weight: 700;}
QLabel[role="badge"] {color: #ddd; border: 1px solid #444; border-radius: 15px; padding: 8px 16px;}
QPushButton {background: #111; color: #eee; border: 1px solid #383838; border-radius: 13px; padding: 8px 18px; font-weight: 600;}
QPushButton:hover {background: #242424; border-color: #888;}
QPushButton:pressed {background: #333;}
QPushButton#primary {background: #f5f5f5; color: #050505; border: none; font-size: 18px;}
QPushButton#primary:hover {background: #ccc;}
QPushButton:disabled {color: #555; background: #080808; border-color: #222;}
QSpinBox,QComboBox {background: #161616; border: 1px solid #444; border-radius: 9px; padding: 8px; min-height: 28px; font-size: 21px;}
QSpinBox:disabled,QComboBox:disabled {background: #080808; color: #444; border-color: #222;}
QLabel:disabled {color: #777;}
QCheckBox {spacing: 9px; background: transparent;}
QCheckBox::indicator {width: 19px; height: 19px; border: 1px solid #666; border-radius: 5px; background: #111;}
QCheckBox::indicator:checked {background: #eee; border: 4px solid #777;}
QCheckBox:disabled {color:#444;}
QTextEdit {background:#080808; color:#ddd; border:1px solid #333; border-radius:10px; padding:10px; font-family:monospace; font-size:12px;}
QDialog {background:#000;}
QToolTip {background:#222; color:#fff; border:1px solid #555;}
)";
} // namespace
TimeInput::TimeInput(QString name, QWidget *parent) : QFrame(parent) {
    setObjectName(name);
    // Keep numeric clock sections in HH:MM order in both interface languages.
    setLayoutDirection(Qt::LeftToRight);
    auto *l = new QHBoxLayout(this);
    l->setContentsMargins(0, 0, 0, 0);
    l->setSpacing(6);
    hour = new TwoDigitSpin;
    hour->setObjectName(name + "Hour");
    hour->setRange(1, 12);
    hour->setWrapping(true);
    hour->setFixedWidth(78);
    hour->setLocale(QLocale::c());
    minute = new TwoDigitSpin;
    minute->setObjectName(name + "Minute");
    minute->setRange(0, 59);
    minute->setWrapping(true);
    minute->setFixedWidth(78);
    minute->setLocale(QLocale::c());
    minute->setDisplayIntegerBase(10);
    period = new QComboBox;
    period->addItems({"AM", "PM"});
    period->setFixedWidth(85);
    l->addWidget(hour);
    l->addWidget(label(":"));
    l->addWidget(minute);
    l->addWidget(period);
    for (auto *s : {hour, minute})
        connect(s, &QSpinBox::valueChanged, this, [this] {
            if (changed)
                changed();
        });
    connect(period, &QComboBox::currentIndexChanged, this, [this] {
        if (changed)
            changed();
    });
}
QString TimeInput::value() const {
    return QString("%1:%2")
        .arg(hour->value() % 12 + period->currentIndex() * 12, 2, 10, QChar('0'))
        .arg(minute->value(), 2, 10, QChar('0'));
}
void TimeInput::setValue(QString hhmm) {
    auto t = QTime::fromString(hhmm, "HH:mm");
    if (!t.isValid())
        return;
    hour->setValue(t.hour() % 12 == 0 ? 12 : t.hour() % 12);
    minute->setValue(t.minute());
    period->setCurrentIndex(t.hour() >= 12 ? 1 : 0);
}
QString Window::text(QString en, QString ar) const {
    return arabic_ ? ar : en;
}
Window::Window(Client &client, QString file)
    : client_(client),
      settings_(file.isEmpty()
                    ? QStandardPaths::writableLocation(QStandardPaths::AppConfigLocation) +
                          "/preferences.ini"
                    : file,
                QSettings::IniFormat) {
    setWindowTitle("RTC Sleeper");
    setWindowIcon(QIcon::fromTheme("sleep-scheduler"));
    setObjectName("mainWindow");
    setFixedSize(1000, 720);
    setStyleSheet(appStyle);
    arabic_ = settings_.value("language", "en").toString() == "ar";
    last_ = QJsonDocument::fromJson(settings_.value("lastConfig").toByteArray()).object();
    if (last_.isEmpty())
        last_ = {{"sleep_time", "02:30"},
                 {"wake_time", "08:00"},
                 {"now", false},
                 {"days", QJsonArray{}}};
    try {
        state_ = client_.status();
    } catch (const std::exception &e) {
        state_ = idleState();
        state_["message"] = e.what();
    }
    build();
    auto *timer = new QTimer(this);
    connect(timer, &QTimer::timeout, this, &Window::refresh);
    timer->start(1000);
}
void Window::build() {
    building_ = true;
    if (layout()) {
        auto *old = layout();
        while (auto *item = old->takeAt(0)) {
            delete item->widget();
            delete item;
        }
        delete old;
    }
    days_.clear();
    setLayoutDirection(arabic_ ? Qt::RightToLeft : Qt::LeftToRight);
    auto *root = new QVBoxLayout(this);
    root->setContentsMargins(34, 26, 34, 24);
    root->setSpacing(20);
    auto *header = new QHBoxLayout;
    auto *titles = new QVBoxLayout;
    titles->addWidget(label("☾  RTC Sleeper", "title"));
    titles->addWidget(
        label(text("Quiet sleep. Wake on time.", "سكون هادئ. إيقاظ في الموعد."), "muted"));
    header->addLayout(titles);
    header->addStretch();
    auto *settings = button(text("⚙  Settings", "⚙  الإعدادات"));
    connect(settings, &QPushButton::clicked, this, &Window::settingsDialog);
    header->addWidget(settings);
    root->addLayout(header);
    pages_ = new QStackedWidget;
    form_ = makeForm();
    dashboard_ = makeDashboard();
    pages_->addWidget(form_);
    pages_->addWidget(dashboard_);
    root->addWidget(pages_, 1);
    root->addWidget(label(text("Closing this window keeps an activated schedule running.",
                               "غلق النافذة يترك الجدول المفعّل يعمل."),
                          "muted"));
    setConfig(last_);
    building_ = false;
    if (state_["config"].isObject() && !editing_)
        setConfig(state_["config"].toObject());
    bool active = state_["config"].isObject();
    pages_->setCurrentWidget(active && !editing_ ? dashboard_ : form_);
    cancelEdit_->setVisible(editing_);
    updateForm();
    updateDashboard();
}
QWidget *Window::makeForm() {
    auto *w = new QWidget;
    auto *l = new QVBoxLayout(w);
    l->setContentsMargins(0, 0, 0, 0);
    l->setSpacing(13);
    auto card = [&](QString title, QString hint) -> QHBoxLayout * {
        auto *f = new QFrame;
        f->setProperty("role", "card");
        auto *row = new QHBoxLayout(f);
        row->setContentsMargins(22, 16, 22, 16);
        auto *words = new QVBoxLayout;
        words->addWidget(label(title, "heading"));
        if (!hint.isEmpty())
            words->addWidget(label(hint, "muted"));
        row->addLayout(words);
        row->addStretch();
        l->addWidget(f);
        return row;
    };
    auto *n = card(text("☾  Sleep Now", "☾  السكون الآن"),
                   text("Verify wake time, then sleep in 5 seconds.",
                        "تحقق من موعد الإيقاظ، ثم ادخل السكون بعد 5 ثوانٍ."));
    now_ = new QCheckBox;
    now_->setObjectName("sleepNow");
    n->addWidget(now_);
    connect(now_, &QCheckBox::toggled, this, [this] { updateForm(); });
    auto *s = card(text("Sleep at", "السكون الساعة"), "");
    sleepRow_ = s->parentWidget();
    sleepRow_->setObjectName("sleepAtRow");
    sleep_ = new TimeInput("sleepAt");
    s->addWidget(sleep_);
    auto *wake = card(text("Wake up at", "الإيقاظ الساعة"), "");
    wake_ = new TimeInput("wakeAt");
    wake->addWidget(wake_);
    auto *repeat = card(text("Repeat", "التكرار"), "");
    repeatRow_ = repeat->parentWidget();
    auto en = QStringList{"M", "T", "W", "T", "F", "S", "S"};
    auto ar = QStringList{"إثن", "ثلا", "أرب", "خمي", "جمع", "سبت", "أحد"};
    for (int i = 0; i < 7; i++) {
        auto *d = new QCheckBox(arabic_ ? ar[i] : en[i]);
        d->setToolTip(QLocale(QLocale::English).dayName(i + 1));
        days_.append(d);
        repeat->addWidget(d);
        connect(d, &QCheckBox::toggled, this, [this] { updateForm(); });
    }
    auto *d = card(text("Sleep duration", "مدة السكون"), "");
    duration_ = label("", "duration");
    d->addWidget(duration_);
    auto *actions = new QHBoxLayout;
    auto *save = button(text("Schedule Sleep", "تفعيل جدول السكون"), "primary");
    save->setObjectName("primary");
    save->setMinimumHeight(58);
    actions->addWidget(save, 1);
    connect(save, &QPushButton::clicked, this, &Window::schedule);
    cancelEdit_ = button(text("Cancel Editing", "إلغاء التعديل"), "cancelEditing");
    actions->addWidget(cancelEdit_);
    connect(cancelEdit_, &QPushButton::clicked, this, &Window::cancelEdit);
    auto *clear = button(text("Reset Fields", "إعادة الحقول"));
    actions->addWidget(clear);
    connect(clear, &QPushButton::clicked, this, [this] { setConfig(last_); });
    l->addLayout(actions);
    sleep_->changed = [this] { updateForm(); };
    wake_->changed = [this] { updateForm(); };
    return w;
}
QWidget *Window::makeDashboard() {
    auto *w = new QWidget;
    auto *l = new QVBoxLayout(w);
    l->setContentsMargins(0, 0, 0, 0);
    badge_ = label("", "badge");
    l->addWidget(badge_, 0, Qt::AlignLeft);
    l->addStretch();
    auto *row = new QHBoxLayout;
    auto *info = new QFrame;
    info->setProperty("role", "card");
    info->setFixedWidth(285);
    auto *il = new QVBoxLayout(info);
    il->setContentsMargins(23, 24, 23, 24);
    il->setSpacing(16);
    il->addWidget(label(text("Scheduled Trigger", "تفاصيل الجدول"), "heading"));
    summary_ = label("");
    summary_->setWordWrap(true);
    il->addWidget(summary_);
    il->addWidget(label(text("Will sleep in:", "السكون خلال:"), "muted"));
    countdown_ = label("", "heading");
    countdown_->setObjectName("willSleepIn");
    il->addWidget(countdown_);
    row->addWidget(info);
    row->addStretch();
    auto *ring = new Ring;
    auto *rl = new QVBoxLayout(ring);
    rl->setContentsMargins(38, 50, 38, 50);
    rl->addStretch();
    auto *moon = label("☾", "title");
    moon->setAlignment(Qt::AlignCenter);
    rl->addWidget(moon);
    ringText_ = label("", "heading");
    ringText_->setWordWrap(true);
    ringText_->setAlignment(Qt::AlignCenter);
    rl->addWidget(ringText_);
    rl->addStretch();
    row->addWidget(ring);
    row->addStretch();
    auto *actions = new QVBoxLayout;
    auto *edit = button(text("Edit Schedule", "تعديل الجدول"), "primary");
    actions->addWidget(edit);
    connect(edit, &QPushButton::clicked, this, &Window::beginEdit);
    pause_ = button(text("Pause", "تعليق"));
    actions->addWidget(pause_);
    connect(pause_, &QPushButton::clicked, this, [this] {
        operation(state_["status"] == "paused" ? "resume" : "pause",
                  state_["status"] == "paused" ? QJsonObject{{"replace_alarm", false}}
                                               : QJsonObject{});
    });
    auto *remove = button(text("Remove", "إلغاء الجدول"), "removeSchedule");
    actions->addWidget(remove);
    connect(remove, &QPushButton::clicked, this, &Window::removeSchedule);
    row->addLayout(actions);
    l->addLayout(row);
    l->addStretch();
    next_ = label("", "badge");
    next_->setAlignment(Qt::AlignCenter);
    next_->setWordWrap(true);
    l->addWidget(next_);
    return w;
}
QJsonObject Window::config() const {
    QJsonArray days;
    if (!now_->isChecked())
        for (int i = 0; i < days_.size(); i++)
            if (days_[i]->isChecked())
                days.append(i);
    return {{"sleep_time", sleep_->value()},
            {"wake_time", wake_->value()},
            {"now", now_->isChecked()},
            {"days", days}};
}
void Window::setConfig(QJsonObject c) {
    bool was = building_;
    building_ = true;
    sleep_->setValue(c["sleep_time"].toString());
    wake_->setValue(c["wake_time"].toString());
    now_->setChecked(c["now"].toBool());
    auto ds = c["days"].toArray();
    for (int i = 0; i < days_.size(); i++)
        days_[i]->setChecked(ds.contains(i));
    building_ = was;
    updateForm();
}
void Window::remember(QJsonObject c) {
    last_ = c;
    settings_.setValue("lastConfig", QJsonDocument(c).toJson(QJsonDocument::Compact));
    settings_.sync();
}
void Window::updateForm() {
    if (building_ || !sleepRow_)
        return;
    sleepRow_->setEnabled(!now_->isChecked());
    repeatRow_->setEnabled(!now_->isChecked());
    auto *effect = qobject_cast<QGraphicsOpacityEffect *>(sleepRow_->graphicsEffect());
    if (!effect) {
        effect = new QGraphicsOpacityEffect(sleepRow_);
        sleepRow_->setGraphicsEffect(effect);
    }
    effect->setOpacity(now_->isChecked() ? 0.55 : 1.0);
    try {
        auto c = nextCycle(config(), QDateTime::currentSecsSinceEpoch());
        duration_->setText(remaining(c.second - c.first, arabic_, false));
    } catch (const std::exception &) {
        duration_->setText(text("Choose different times", "اختار مواعيد مختلفة"));
    }
}
void Window::switchPage(QWidget *page, bool animate) {
    if (pages_->currentWidget() == page)
        return;
    pages_->setCurrentWidget(page);
    if (!animate)
        return;
    auto *effect = new QGraphicsOpacityEffect(page);
    page->setGraphicsEffect(effect);
    auto *a = new QPropertyAnimation(effect, "opacity", effect);
    a->setDuration(280);
    a->setStartValue(0.0);
    a->setEndValue(1.0);
    a->setEasingCurve(QEasingCurve::OutCubic);
    a->start(QAbstractAnimation::DeleteWhenStopped);
}
void Window::updateDashboard() {
    if (!badge_)
        return;
    auto status = state_["status"].toString();
    auto english = QMap<QString, QString>{
        {"scheduled", "Schedule Active"}, {"paused", "Schedule Paused"},
        {"sleeping", "Entering Sleep"},   {"conflict", "RTC Conflict — Sleep Blocked"},
        {"error", "Needs Attention"},     {"idle", "Ready"}};
    auto arabic = QMap<QString, QString>{
        {"scheduled", "الجدول مفعّل"},    {"paused", "الجدول معلّق"},
        {"sleeping", "جارٍ دخول السكون"}, {"conflict", "تعارض RTC — السكون ممنوع"},
        {"error", "يحتاج مراجعة"},       {"idle", "جاهز"}};
    badge_->setText((arabic_ ? arabic : english).value(status, status));
    auto c = state_["config"].toObject();
    summary_->setText(text("Sleep at", "السكون الساعة") + "\n" + timeLabel(state_["next_sleep"]) +
                      "\n\n" + text("Wake up at", "الإيقاظ الساعة") + "\n" +
                      timeLabel(state_["next_wake"]));
    countdown_->setText(status == "scheduled" ? remaining(state_["next_sleep"].toInteger() -
                                                              QDateTime::currentSecsSinceEpoch(),
                                                          arabic_)
                                              : "—");
    ringText_->setText(
        badge_->text() + "\n\n" + text("Sleep duration", "مدة السكون") + "\n" +
        (state_["next_wake"].isDouble()
             ? remaining(state_["next_wake"].toInteger() - state_["next_sleep"].toInteger(),
                         arabic_, false)
             : "—"));
    auto *ring = static_cast<Ring *>(ringText_->parentWidget());
    auto total = state_["next_sleep"].toInteger() - state_["scheduled_at"].toInteger();
    ring->progress =
        status == "scheduled" && total > 0
            ? 1.0 - double(state_["next_sleep"].toInteger() - QDateTime::currentSecsSinceEpoch()) /
                        total
            : 0;
    ring->update();
    pause_->setText(status == "paused" ? text("Resume", "استئناف") : text("Pause", "تعليق"));
    pause_->setEnabled(status != "sleeping");
    next_->setText(state_["message"].toString().isEmpty()
                       ? text("Next action: Sleep at ", "الإجراء القادم: السكون الساعة ") +
                             timeLabel(state_["next_sleep"])
                       : state_["message"].toString());
}
void Window::refresh() {
    try {
        auto fresh = client_.status();
        auto old = state_["status"].toString();
        state_ = fresh;
        if (state_["config"].isObject()) {
            if (!editing_)
                remember(state_["config"].toObject());
            if (!editing_ && pages_->currentWidget() == form_)
                switchPage(dashboard_);
        } else if (old != "idle") {
            editing_ = false;
            setConfig(last_);
            cancelEdit_->hide();
            switchPage(form_);
        }
        updateDashboard();
        if (pages_->currentWidget() == form_)
            updateForm();
    } catch (const std::exception &e) {
        next_->setText(e.what());
    }
}
bool Window::operation(QString op, QJsonObject p) {
    try {
        state_ = client_.request(op, p, userActor("gui"));
        updateDashboard();
        return true;
    } catch (const std::exception &e) {
        QMessageBox::warning(this, text("Schedule could not be changed", "تعذّر تغيير الجدول"),
                             QString::fromUtf8(e.what()));
        return false;
    }
}
void Window::schedule() {
    auto c = config();
    try {
        validateConfig(c);
    } catch (const std::exception &e) {
        QMessageBox::warning(this, text("Check times", "راجع المواعيد"), e.what());
        return;
    }
    bool replace = false;
    try {
        auto live = client_.status();
        if (!live["actual_alarm"].isNull() && live["actual_alarm"] != live["expected_alarm"]) {
            if (QMessageBox::question(
                    this, text("Replace external RTC alarm?", "استبدال منبّه RTC خارجي؟"),
                    text("Another program set a wake alarm. Replace it with this schedule?",
                         "برنامج آخر ضبط منبّه إيقاظ. هل تريد استبداله بهذا الجدول؟"),
                    QMessageBox::Yes | QMessageBox::Cancel,
                    QMessageBox::Cancel) != QMessageBox::Yes)
                return;
            replace = true;
        }
    } catch (const std::exception &e) {
        QMessageBox::warning(this, "RTC", e.what());
        return;
    }
    if (operation("schedule", {{"config", c}, {"replace_alarm", replace}})) {
        remember(c);
        editing_ = false;
        cancelEdit_->hide();
        switchPage(dashboard_);
    }
}
void Window::beginEdit() {
    snapshot_ = last_;
    editing_ = true;
    if (state_["config"].isObject())
        setConfig(state_["config"].toObject());
    cancelEdit_->show();
    switchPage(form_);
}
void Window::cancelEdit() {
    editing_ = false;
    setConfig(snapshot_.isEmpty() ? last_ : snapshot_);
    cancelEdit_->hide();
    switchPage(state_["config"].isObject() ? dashboard_ : form_);
}
void Window::removeSchedule() {
    if (state_["config"].isObject())
        remember(state_["config"].toObject());
    if (operation("cancel")) {
        editing_ = false;
        setConfig(last_);
        cancelEdit_->hide();
        switchPage(form_);
    }
}
void Window::logsDialog() {
    QDialog d(this);
    d.setWindowTitle(text("Activity Logs", "سجل النشاط"));
    d.resize(840, 540);
    auto *l = new QVBoxLayout(&d);
    l->addWidget(label(
        text("Agent/model labels are self-declared; OS UID is recorded separately.",
             "أسماء الـagent والموديل بيانات يقدّمها صاحب الطلب؛ مستخدم النظام يُسجّل بشكل منفصل."),
        "muted"));
    auto *body = new QTextEdit;
    body->setReadOnly(true);
    l->addWidget(body);
    auto *b = button(text("Refresh", "تحديث"));
    l->addWidget(b);
    auto load = [&] {
        try {
            body->setPlainText(
                QString::fromUtf8(QJsonDocument(client_.logs()).toJson(QJsonDocument::Indented)));
        } catch (const std::exception &e) {
            body->setPlainText(e.what());
        }
    };
    connect(b, &QPushButton::clicked, &d, load);
    load();
    d.exec();
}
void Window::settingsDialog() {
    QDialog d(this);
    d.setWindowTitle(text("Settings", "الإعدادات"));
    d.setFixedSize(440, 260);
    auto *l = new QVBoxLayout(&d);
    l->setContentsMargins(24, 24, 24, 24);
    l->addWidget(label(text("Language", "اللغة"), "heading"));
    auto *lang = new QComboBox;
    lang->addItems({"English", "العربية"});
    lang->setCurrentIndex(arabic_ ? 1 : 0);
    l->addWidget(lang);
    auto *logs = button(text("Activity Logs", "سجل النشاط"));
    l->addWidget(logs);
    connect(logs, &QPushButton::clicked, this, &Window::logsDialog);
    auto *close = new QDialogButtonBox(QDialogButtonBox::Save | QDialogButtonBox::Cancel);
    l->addWidget(close);
    connect(close, &QDialogButtonBox::accepted, &d, &QDialog::accept);
    connect(close, &QDialogButtonBox::rejected, &d, &QDialog::reject);
    if (d.exec() == QDialog::Accepted && arabic_ != (lang->currentIndex() == 1)) {
        auto draft = config();
        arabic_ = lang->currentIndex() == 1;
        settings_.setValue("language", arabic_ ? "ar" : "en");
        build();
        if (editing_ || !state_["config"].isObject())
            setConfig(draft);
    }
}
} // namespace Sleeper

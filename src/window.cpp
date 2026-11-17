#include "window.h"
#include <QApplication>
#include <QDateTime>
#include <QDesktopServices>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDir>
#include <QElapsedTimer>
#include <QGraphicsOpacityEffect>
#include <QHBoxLayout>
#include <QJsonDocument>
#include <QLineEdit>
#include <QLocale>
#include <QMessageBox>
#include <QPainter>
#include <QProcess>
#include <QPropertyAnimation>
#include <QScrollArea>
#include <QStandardPaths>
#include <QTextEdit>
#include <QTimer>
#include <QToolButton>
#include <QUrl>
#include <QVBoxLayout>
#include <QVariantAnimation>

namespace Sleeper {
namespace {
class DigitAnimation : public QWidget {
  public:
    QString previous, next;
    qreal progress = 0;
    int direction = 1;
    explicit DigitAnimation(QWidget *parent) : QWidget(parent) {
        setAttribute(Qt::WA_TransparentForMouseEvents);
        hide();
    }
    void paintEvent(QPaintEvent *) override {
        QPainter p(this);
        p.setRenderHint(QPainter::TextAntialiasing);
        p.fillRect(rect(), QColor("#161616"));
        p.setClipRect(rect());
        p.setPen(QColor("#f5f5f5"));
        p.setFont(font());
        auto previousRect = QRectF(rect()).translated(0, -direction * progress * height());
        auto nextRect = previousRect.translated(0, direction * height());
        p.drawText(previousRect, Qt::AlignCenter, previous);
        p.drawText(nextRect, Qt::AlignCenter, next);
    }
};
class TwoDigitSpin : public QSpinBox {
    int previous_ = 0, stepDirection_ = 0;
    DigitAnimation *overlay_;
    QVariantAnimation animation_;

  public:
    TwoDigitSpin() : overlay_(new DigitAnimation(this)) {
        setButtonSymbols(QAbstractSpinBox::NoButtons);
        setAlignment(Qt::AlignCenter);
        animation_.setDuration(115);
        animation_.setStartValue(0.0);
        animation_.setEndValue(1.0);
        animation_.setEasingCurve(QEasingCurve::OutCubic);
        connect(&animation_, &QVariantAnimation::valueChanged, this, [this](QVariant value) {
            overlay_->progress = value.toReal();
            overlay_->update();
        });
        connect(&animation_, &QVariantAnimation::finished, overlay_, &QWidget::hide);
        connect(this, &QSpinBox::valueChanged, this, [this](int value) {
            const int old = previous_;
            previous_ = value;
            if (!isVisible() || !isEnabled())
                return;
            animation_.stop();
            overlay_->setObjectName(objectName() + "DigitAnimation");
            overlay_->setGeometry(lineEdit()->geometry());
            overlay_->setFont(lineEdit()->font());
            overlay_->previous = textFromValue(old);
            overlay_->next = textFromValue(value);
            overlay_->direction = stepDirection_ ? stepDirection_ : (value > old ? 1 : -1);
            overlay_->progress = 0;
            overlay_->show();
            overlay_->raise();
            animation_.start();
        });
    }

  protected:
    bool event(QEvent *event) override {
        if (event->type() == QEvent::Hide ||
            (event->type() == QEvent::EnabledChange && !isEnabled())) {
            animation_.stop();
            overlay_->hide();
        }
        return QSpinBox::event(event);
    }
    void stepBy(int steps) override {
        stepDirection_ = steps >= 0 ? 1 : -1;
        QSpinBox::stepBy(steps);
        stepDirection_ = 0;
    }
    QString textFromValue(int value) const override {
        return QString::number(value).rightJustified(2, '0');
    }
};
class HoldArrow : public QToolButton {
    QTimer repeat_;
    QElapsedTimer held_;
    QSpinBox *field_;
    int direction_;
    void step() {
        if (direction_ > 0)
            field_->stepUp();
        else
            field_->stepDown();
    }

  public:
    HoldArrow(QSpinBox *field, int direction) : field_(field), direction_(direction) {
        setFixedSize(36, 23);
        setCursor(Qt::PointingHandCursor);
        setAutoRepeat(false);
        setObjectName(field->objectName() + (direction > 0 ? "Increase" : "Decrease"));
        const auto description = direction > 0 ? "Increase / زيادة" : "Decrease / تقليل";
        setToolTip(description);
        setAccessibleName(description);
        repeat_.setTimerType(Qt::PreciseTimer);
        connect(this, &QToolButton::pressed, this, [this] {
            held_.start();
            step();
            repeat_.start(400);
        });
        connect(this, &QToolButton::released, &repeat_, &QTimer::stop);
        connect(&repeat_, &QTimer::timeout, this, [this] {
            if (!isDown() || !isEnabled() || !isVisible()) {
                repeat_.stop();
                return;
            }
            step();
            // Ramp gently from about 4 steps/s to a hard cap of 7 steps/s.
            repeat_.start(qMax(140, 280 - int(held_.elapsed() / 25)));
        });
    }

  protected:
    bool event(QEvent *event) override {
        if (event->type() == QEvent::Hide || event->type() == QEvent::UngrabMouse ||
            (event->type() == QEvent::EnabledChange && !isEnabled()))
            repeat_.stop();
        return QToolButton::event(event);
    }
    void paintEvent(QPaintEvent *event) override {
        QToolButton::paintEvent(event);
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        p.setPen(QPen(isEnabled() ? QColor("#eeeeee") : QColor("#444444"), 1.8, Qt::SolidLine,
                      Qt::RoundCap, Qt::RoundJoin));
        const qreal middle = height() / 2.0;
        const qreal tip = middle - direction_ * 3;
        const qreal edge = middle + direction_ * 3;
        p.drawPolyline(QPolygonF{QPointF(12, edge), QPointF(18, tip), QPointF(24, edge)});
    }
};
QWidget *steppingField(QSpinBox *field) {
    auto *container = new QWidget;
    auto *column = new QVBoxLayout(container);
    column->setContentsMargins(0, 0, 0, 0);
    column->setSpacing(3);
    auto *arrows = new QHBoxLayout;
    arrows->setSpacing(6);
    arrows->addWidget(new HoldArrow(field, 1));
    arrows->addWidget(new HoldArrow(field, -1));
    column->addLayout(arrows);
    column->addWidget(field);
    return container;
}
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
QTextEdit {background:#080808; color:#ddd; border:1px solid #333; border-radius:10px; padding:10px; font-family:monospace; font-size:12px;}
QDialog {background:#000;}
QToolTip {background:#222; color:#fff; border:1px solid #555;}
QToolButton {background:#151515; border:1px solid #444; border-radius:6px; padding:0;}
QToolButton:hover {background:#303030; border-color:#999;}
QToolButton:pressed {background:#454545; border-color:#eee;}
QToolButton:disabled {background:#080808; border-color:#222;}
)";
} // namespace
ToggleSwitch::ToggleSwitch(QWidget *parent) : QAbstractButton(parent) {
    setCheckable(true);
    setFocusPolicy(Qt::StrongFocus);
    setCursor(Qt::PointingHandCursor);
    setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
    animation_.setDuration(180);
    animation_.setEasingCurve(QEasingCurve::InOutCubic);
    connect(&animation_, &QVariantAnimation::valueChanged, this, [this](QVariant value) {
        position_ = value.toReal();
        update();
    });
    connect(this, &QAbstractButton::toggled, this, [this](bool on) {
        animation_.stop();
        const qreal target = on ? 1.0 : 0.0;
        if (!isVisible()) {
            position_ = target;
            update();
            return;
        }
        animation_.setStartValue(position_);
        animation_.setEndValue(target);
        animation_.start();
    });
}
QSize ToggleSwitch::sizeHint() const {
    return QSize(56, 32);
}
void ToggleSwitch::hideEvent(QHideEvent *event) {
    animation_.stop();
    position_ = isChecked() ? 1.0 : 0.0;
    QAbstractButton::hideEvent(event);
}
void ToggleSwitch::paintEvent(QPaintEvent *) {
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing);
    const QRectF track(2, (height() - 28) / 2.0, width() - 4, 28);
    const int shade = qRound(30 + position_ * 195);
    p.setPen(QPen(isEnabled() ? QColor("#666666") : QColor("#333333"), 1));
    p.setBrush(isEnabled() ? QColor(shade, shade, shade) : QColor("#171717"));
    p.drawRoundedRect(track, 14, 14);
    // The thumb always travels horizontally; RTL mirrors which side is on.
    const qreal position = layoutDirection() == Qt::RightToLeft ? 1.0 - position_ : position_;
    const qreal x = track.left() + 4 + position * (track.width() - 28);
    const int thumb = qRound(245 - position_ * 220);
    p.setPen(Qt::NoPen);
    p.setBrush(isEnabled() ? QColor(thumb, thumb, thumb) : QColor("#555555"));
    p.drawEllipse(QRectF(x, track.top() + 4, 20, 20));
    if (hasFocus()) {
        p.setBrush(Qt::NoBrush);
        p.setPen(QPen(QColor("#aaaaaa"), 1, Qt::DotLine));
        p.drawRoundedRect(track.adjusted(-2, -2, 2, 2), 16, 16);
    }
}
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
    l->addWidget(steppingField(hour));
    l->addWidget(label(":"));
    l->addWidget(steppingField(minute));
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
    l->setSpacing(10);
    auto card = [&](QString title, QString hint) -> QHBoxLayout * {
        auto *f = new QFrame;
        f->setProperty("role", "card");
        auto *row = new QHBoxLayout(f);
        row->setContentsMargins(22, 12, 22, 12);
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
    now_ = new ToggleSwitch;
    now_->setObjectName("sleepNow");
    now_->setAccessibleName(text("Sleep Now", "السكون الآن"));
    n->addWidget(now_);
    connect(now_, &QAbstractButton::toggled, this, [this] { updateForm(); });
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
        auto *d = new ToggleSwitch;
        d->setObjectName(QString("repeatDay%1").arg(i));
        auto dayName = QLocale(arabic_ ? QLocale::Arabic : QLocale::English).dayName(i + 1);
        d->setToolTip(dayName);
        d->setAccessibleName(dayName);
        days_.append(d);
        auto *day = new QVBoxLayout;
        day->setSpacing(4);
        auto *caption = label(arabic_ ? ar[i] : en[i], "muted");
        caption->setAlignment(Qt::AlignCenter);
        day->addWidget(caption);
        day->addWidget(d);
        repeat->addLayout(day);
        connect(d, &QAbstractButton::toggled, this, [this] { updateForm(); });
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
void Window::remoteDialog() {
    QDialog d(this);
    d.setWindowTitle(text("Remote Control · Android", "التحكم عن بعد · أندرويد"));
    d.resize(640, 700);
    auto *l = new QVBoxLayout(&d);
    auto *status = label("", "muted");
    status->setWordWrap(true);
    l->addWidget(status);
    auto *signIn = button(text("Sign in to private network", "تسجيل الدخول للشبكة الخاصة"));
    l->addWidget(signIn);
    auto *generate = button(text("New pairing code", "إنشاء كود اقتران"), "primary");
    l->addWidget(generate);
    auto *qr = new QLabel;
    qr->setAlignment(Qt::AlignCenter);
    l->addWidget(qr);
    auto *phrase = new QTextEdit;
    phrase->setReadOnly(true);
    phrase->setMaximumHeight(130);
    l->addWidget(phrase);
    l->addWidget(
        label(text("Scan the QR or enter the phrase in the Android app. Codes expire in 10 minutes "
                   "and work once.",
                   "امسح QR أو أدخل العبارة في تطبيق أندرويد. الكود صالح 10 دقائق ولمرة واحدة."),
              "muted"));
    auto *refresh = button(text("Refresh connection", "تحديث الاتصال"));
    l->addWidget(refresh);
    auto *revoke = button(text("Revoke all paired phones", "إلغاء صلاحية كل الهواتف"));
    l->addWidget(revoke);
    QString loginUrl;
    auto request = [&](const QString &op) {
        auto executable = QStandardPaths::findExecutable("rtc-sleeper-remote");
        if (executable.isEmpty())
            executable = QDir::homePath() + "/.local/bin/rtc-sleeper-remote";
        QProcess p;
        p.start(executable, {op});
        if (!p.waitForStarted(1500) || !p.waitForFinished(9000)) {
            p.kill();
            p.waitForFinished();
            throw std::runtime_error(
                "Install/start the remote service: bash scripts/install-remote.sh");
        }
        auto bytes = p.readAllStandardOutput();
        if (p.exitCode() != 0)
            throw std::runtime_error(p.readAllStandardError().toStdString());
        auto result = QJsonDocument::fromJson(bytes);
        if (!result.isObject())
            throw std::runtime_error("Invalid response from remote service");
        return result.object();
    };
    auto update = [&] {
        try {
            auto s = request("status");
            loginUrl = s["AuthURL"].toString();
            bool ready = s["BackendState"] == "Running";
            status->setText(
                ready ? text("Private network connected. Ready to pair.",
                             "الشبكة الخاصة متصلة. جاهز للاقتران.")
                      : text("Sign in with the same Tailscale account used in the Android app.",
                             "سجّل الدخول بنفس حساب Tailscale المستخدم في تطبيق أندرويد."));
            generate->setEnabled(ready);
            signIn->setEnabled(!ready);
        } catch (const std::exception &e) {
            status->setText(e.what());
            generate->setEnabled(false);
        }
    };
    connect(refresh, &QPushButton::clicked, &d, update);
    connect(signIn, &QPushButton::clicked, &d, [&] {
        try {
            request("login");
            update();
            if (loginUrl.startsWith("https://login.tailscale.com/"))
                QDesktopServices::openUrl(QUrl(loginUrl));
            else
                status->setText(text("Login starting; refresh in a moment.",
                                     "تسجيل الدخول يبدأ؛ اضغط تحديث بعد لحظات."));
        } catch (const std::exception &e) {
            status->setText(e.what());
        }
    });
    connect(generate, &QPushButton::clicked, &d, [&] {
        try {
            auto ticket = request("pair");
            QPixmap image;
            image.loadFromData(QByteArray::fromBase64(ticket["qr_png"].toString().toUtf8()), "PNG");
            qr->setPixmap(image.scaled(300, 300, Qt::KeepAspectRatio, Qt::SmoothTransformation));
            phrase->setPlainText(ticket["phrase"].toString());
        } catch (const std::exception &e) {
            status->setText(e.what());
        }
    });
    connect(revoke, &QPushButton::clicked, &d, [&] {
        if (QMessageBox::question(&d, text("Revoke phones?", "إلغاء صلاحية الهواتف؟"),
                                  text("All paired phones will need a new pairing code.",
                                       "كل الهواتف ستحتاج كود اقتران جديد.")) != QMessageBox::Yes)
            return;
        try {
            request("revoke");
            qr->clear();
            phrase->clear();
            status->setText(text("Phone access revoked.", "تم إلغاء صلاحية الهواتف."));
        } catch (const std::exception &e) {
            status->setText(e.what());
        }
    });
    update();
    d.exec();
}
void Window::settingsDialog() {
    QDialog d(this);
    d.setWindowTitle(text("Settings", "الإعدادات"));
    d.setFixedSize(440, 330);
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
    auto *remote = button(text("Remote Control · Android", "التحكم عن بعد · أندرويد"));
    l->addWidget(remote);
    connect(remote, &QPushButton::clicked, this, &Window::remoteDialog);
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

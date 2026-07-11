#pragma once

#include <QObject>
#include <QQuickItem>
#include <QUrl>
#include <QtQml/qqmlregistration.h>

class DragOutHelper : public QObject
{
    Q_OBJECT
    QML_ELEMENT

public:
    using QObject::QObject;

    // Blocks in a nested event loop until the drag finishes; must be called
    // while a mouse button is held (Wayland requires an active grab).
    Q_INVOKABLE bool startDrag(QQuickItem *source, const QList<QUrl> &urls);
};

#include "ui/dragouthelper.h"

#include <QDrag>
#include <QMimeData>
#include <QQuickItem>

bool DragOutHelper::startDrag(QQuickItem *source, const QList<QUrl> &urls)
{
    if (!source || urls.isEmpty())
        return false;

    auto *drag = new QDrag(source);
    auto *mime = new QMimeData;
    mime->setUrls(urls);
    drag->setMimeData(mime);

    const Qt::DropAction result =
        drag->exec(Qt::CopyAction | Qt::MoveAction, Qt::CopyAction);
    return result != Qt::IgnoreAction;
}

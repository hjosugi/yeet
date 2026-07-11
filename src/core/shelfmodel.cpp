#include "core/shelfmodel.h"

#include <QFileInfo>

ShelfModel::ShelfModel(QObject *parent)
    : QAbstractListModel(parent)
{
}

int ShelfModel::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : static_cast<int>(m_items.size());
}

QVariant ShelfModel::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_items.size())
        return {};

    const Item &item = m_items.at(index.row());
    switch (role) {
    case FileUrlRole:
        return item.url;
    case DisplayNameRole:
        return item.displayName;
    default:
        return {};
    }
}

QHash<int, QByteArray> ShelfModel::roleNames() const
{
    return {
        { FileUrlRole, "fileUrl" },
        { DisplayNameRole, "displayName" },
    };
}

void ShelfModel::addUrls(const QList<QUrl> &urls)
{
    QList<Item> fresh;
    for (const QUrl &url : urls) {
        if (!url.isValid())
            continue;
        const bool duplicate =
            std::any_of(m_items.cbegin(), m_items.cend(),
                        [&url](const Item &it) { return it.url == url; })
            || std::any_of(fresh.cbegin(), fresh.cend(),
                           [&url](const Item &it) { return it.url == url; });
        if (duplicate)
            continue;

        QString name = url.fileName();
        if (name.isEmpty())
            name = url.toDisplayString(QUrl::PreferLocalFile);
        fresh.append({ url, name });
    }
    if (fresh.isEmpty())
        return;

    const int first = static_cast<int>(m_items.size());
    beginInsertRows({}, first, first + static_cast<int>(fresh.size()) - 1);
    m_items.append(fresh);
    endInsertRows();
    emit countChanged();
}

void ShelfModel::removeAt(int row)
{
    if (row < 0 || row >= m_items.size())
        return;
    beginRemoveRows({}, row, row);
    m_items.removeAt(row);
    endRemoveRows();
    emit countChanged();
    if (m_items.isEmpty())
        emit becameEmpty();
}

void ShelfModel::clear()
{
    if (m_items.isEmpty())
        return;
    beginResetModel();
    m_items.clear();
    endResetModel();
    emit countChanged();
    emit becameEmpty();
}

QList<QUrl> ShelfModel::allUrls() const
{
    QList<QUrl> urls;
    urls.reserve(m_items.size());
    for (const Item &item : m_items)
        urls.append(item.url);
    return urls;
}

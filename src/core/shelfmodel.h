#pragma once

#include <QAbstractListModel>
#include <QUrl>
#include <QtQml/qqmlregistration.h>

class ShelfModel : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)

public:
    enum Roles {
        FileUrlRole = Qt::UserRole + 1,
        DisplayNameRole,
    };

    explicit ShelfModel(QObject *parent = nullptr);

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    Q_INVOKABLE void addUrls(const QList<QUrl> &urls);
    Q_INVOKABLE void removeAt(int row);
    Q_INVOKABLE void clear();
    Q_INVOKABLE QList<QUrl> allUrls() const;

signals:
    void countChanged();
    void becameEmpty();

private:
    struct Item {
        QUrl url;
        QString displayName;
    };
    QList<Item> m_items;
};

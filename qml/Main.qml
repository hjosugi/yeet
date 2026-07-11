import QtQuick
import Yeet

Window {
    id: root
    width: 280
    height: 400
    minimumWidth: 220
    minimumHeight: 240
    visible: true
    title: qsTr("Yeet")
    color: "#26272c"

    ShelfModel { id: shelf }
    DragOutHelper { id: dragOut }

    DropArea {
        id: dropArea
        anchors.fill: parent
        onDropped: drop => {
            if (drop.hasUrls) {
                shelf.addUrls(drop.urls)
                drop.accept(Qt.CopyAction)
            }
        }
    }

    Rectangle {
        anchors.fill: parent
        color: "transparent"
        border.color: dropArea.containsDrag ? "#4b9fff" : "#3b3d45"
        border.width: 2
        radius: 10
    }

    Text {
        anchors.centerIn: parent
        visible: shelf.count === 0
        text: qsTr("Drop files here")
        color: "#8b8d98"
        font.pixelSize: 15
    }

    ListView {
        id: list
        anchors.fill: parent
        anchors.margins: 12
        spacing: 6
        clip: true
        model: shelf

        delegate: Rectangle {
            id: entry

            required property int index
            required property url fileUrl
            required property string displayName

            width: list.width
            height: 44
            radius: 6
            color: hoverArea.pressed ? "#3f424c" : "#33363e"

            Text {
                anchors.verticalCenter: parent.verticalCenter
                anchors.left: parent.left
                anchors.right: removeButton.left
                anchors.margins: 10
                elide: Text.ElideMiddle
                text: entry.displayName
                color: "#e6e7eb"
            }

            Text {
                id: removeButton
                anchors.verticalCenter: parent.verticalCenter
                anchors.right: parent.right
                anchors.rightMargin: 10
                text: "✕"
                color: "#8b8d98"

                TapHandler {
                    onTapped: shelf.removeAt(entry.index)
                }
            }

            MouseArea {
                id: hoverArea
                anchors.fill: parent
                anchors.rightMargin: 28
                property point pressPos

                onPressed: mouse => pressPos = Qt.point(mouse.x, mouse.y)
                onPositionChanged: mouse => {
                    if (!pressed)
                        return
                    const dx = mouse.x - pressPos.x
                    const dy = mouse.y - pressPos.y
                    if (dx * dx + dy * dy < 64)
                        return
                    if (dragOut.startDrag(entry, [entry.fileUrl]))
                        shelf.removeAt(entry.index)
                }
            }
        }
    }
}

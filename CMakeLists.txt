cmake_minimum_required(VERSION 3.21)
project(yeet VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Qt6 6.5 REQUIRED COMPONENTS Quick)
qt_standard_project_setup(REQUIRES 6.5)

qt_add_executable(yeet
    src/main.cpp
)

qt_add_qml_module(yeet
    URI Yeet
    VERSION 1.0
    QML_FILES
        qml/Main.qml
    SOURCES
        src/core/shelfmodel.h
        src/core/shelfmodel.cpp
        src/ui/dragouthelper.h
        src/ui/dragouthelper.cpp
)

# qmltyperegistrar includes SOURCES headers by basename only
target_include_directories(yeet PRIVATE src src/core src/ui)
target_link_libraries(yeet PRIVATE Qt6::Quick)

install(TARGETS yeet RUNTIME DESTINATION bin)

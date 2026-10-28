// Adapts the original contextual tagger and scansion APIs to JSON lines.
#include "lemCore.h"
#include "tagueur.h"
#include "scandeur.h"
#include <QCoreApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QTextStream>

int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    if (argc < 2 || argc > 3) return 2;
    LemCore core(nullptr, QString::fromLocal8Bit(argv[1]));
    Tagueur tagger(nullptr, &core, "en", QString::fromLocal8Bit(argv[1]));
    Scandeur scanner(nullptr, &core, QString::fromLocal8Bit(argv[1]));
    QTextStream input(stdin), output(stdout);
    input.setCodec("UTF-8"); output.setCodec("UTF-8");
    while (!input.atEnd()) {
        auto text = input.readLine();
        QJsonArray values;
        values.append(tagger.tagTexte(text, -1, false, true, true));
        values.append(scanner.scandeTxt(text, 0, true, false));
        if (argc == 3 && QByteArray(argv[2]) == "--accent") {
            for (int mode : {1, 2, 3, 9, 10, 11})
                values.append(scanner.scandeTxt(text, mode, false, false));
        }
        output << QJsonDocument(values).toJson(QJsonDocument::Compact) << '\n';
        output.flush();
    }
}

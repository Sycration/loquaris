// Minimal, line-oriented adapter for comparing the original Qt lemmatizer.
#include "lemCore.h"
#include <QCoreApplication>
#include <QSet>
#include <QTextStream>

int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    if (argc < 2) return 2;
    LemCore core(nullptr, QString::fromLocal8Bit(argv[1]));
    bool sentence_start = false;
    bool forms = false;
    bool tags = false;
    for (int i = 2; i < argc; ++i) {
        if (QByteArray(argv[i]) == "--sentence-start") sentence_start = true;
        else if (QByteArray(argv[i]) == "--extension") core.setExtension(true);
        else if (QByteArray(argv[i]) == "--forms") forms = true;
        else if (QByteArray(argv[i]) == "--tags") tags = true;
        else return 2;
    }
    QTextStream input(stdin), output(stdout);
    input.setCodec("UTF-8");
    output.setCodec("UTF-8");
    while (!input.atEnd()) {
        QString word = input.readLine();
        auto analyses = core.lemmatiseM(word, sentence_start);
        QSet<QString> unique;
        for (auto it = analyses.cbegin(); it != analyses.cend(); ++it) {
            for (const auto &analysis : it.value()) {
                QString value = it.key()->cle() + ":" + QString::number(analysis.morpho);
                if (forms) value += ":" + analysis.grq + ":" + analysis.sufq;
                if (tags) {
                    const auto entries = core.tag(it.key(), analysis.morpho).split(',', Qt::SkipEmptyParts);
                    for (const auto &tag : entries) unique.insert(value + ":" + tag);
                } else unique.insert(value);
            }
        }
        QStringList sorted = unique.values();
        sorted.sort();
        output << sorted.join("\t") << '\n';
    }
}

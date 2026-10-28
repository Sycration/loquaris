// Test-only access to the original's foot search; the production C++ is unmodified.
#include "lemCore.h"
#include <QCoreApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QTextStream>
#define private public
#include "scandeur.h"
#undef private

int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    if (argc != 2) return 2;
    LemCore core(nullptr, QString::fromLocal8Bit(argv[1]));
    Scandeur scanner(nullptr, &core, QString::fromLocal8Bit(argv[1]));
    QTextStream input(stdin), output(stdout);
    input.setCodec("UTF-8"); output.setCodec("UTF-8");
    while (!input.atEnd()) {
        QString html = scanner.scandeTxt(input.readLine(), 0, true, false);
        int start = html.indexOf("&nbsp;<small>");
        int end = start < 0 ? -1 : html.indexOf("</small>", start);
        QJsonArray result;
        if (start < 0 || end < 0) result.append("unavailable");
        else {
            QString schema = html.mid(start + QString("&nbsp;<small>").size(), end - start - QString("&nbsp;<small>").size());
            schema.replace(QString::fromUtf8("∪̲"), "*");
            schema.replace(QString::fromUtf8("∪"), "-");
            schema.replace(QString::fromUtf8("‑"), "+");
            result.append("schema:" + schema);
            if (schema.contains('?') || schema.contains('@')) result.append("indeterminate");
            else {
                int syllables = schema.count('+') + schema.count('-') + schema.count('*');
                int first = 0;
                while (first < schema.size() && !QString("+-*").contains(schema[first])) ++first;
                if (syllables >= 10 && first < schema.size() && schema[first] != '-') {
                    for (const QString &feet : scanner.cherchePieds(6, schema, first, false)) {
                        if (feet.size() != 6) continue;
                        int count = 0;
                        for (QChar foot : feet) {
                            if (QString("SsX").contains(foot)) count += 2;
                            if (QString("Dd").contains(foot)) count += 3;
                            if (QString("YyZ").contains(foot)) ++count;
                        }
                        if (count == syllables) result.append(feet);
                    }
                }
            }
        }
        output << QJsonDocument(result).toJson(QJsonDocument::Compact) << '\n';
        output.flush();
    }
}

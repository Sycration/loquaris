/*                 lemCore.cpp
 *
 *  This file is part of COLLATINUS.
 *
 *  COLLATINUS is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  COLLATINVS is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with COLLATINUS; if not, write to the Free Software
 *  Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA
 *
 * © Yves Ouvrard, 2009 - 2016
 */

/**
 * \file lemmatiseur.cpp
 * \brief module de lemmatisation des formes latines
 */

#include "lemCore.h"

#include <QDebug>
// #include <QElapsedTimer>
// #define DEBOG
// #define VERIF_TRAD

/**
 * \fn LemCore::LemCore (QObject *parent)
 * \brief Constructeur de la classe Lemmat.
 *
 * Il définit quelques constantes, initialise
 * les options à false, et appelle les fonctions
 * de lecture des données : modèles, lexique,
 * traductions et irréguliers.
 */
LemCore::LemCore(QObject *parent, QString resDir) : QObject(parent)
{
    if (resDir == "")
        _resDir = qApp->applicationDirPath() + "/data/";
    else if (resDir.endsWith("/")) _resDir = resDir;
    else _resDir = resDir + "/";
    // options
    _extension = false;
    _extLoaded = false;
    _nbrLoaded = false;
    _cible = "fr en es";
    // Par défaut, la langue cible est le français. L'anglais est le second choix
    // (si une traduction n'existe pas en français, on la cherche en anglais).
    // suffixes
    suffixes.insert("ne", "nĕ");
    suffixes.insert("que", "quĕ");
    suffixes.insert("ue", "vĕ");
    suffixes.insert("ve", "vĕ");
    suffixes.insert("st", "st");
    // assimilations
    ajAssims();
    ajAbrev();
    // contractions
    ajContractions();
    // lecture des morphos
    QDir rep;
    rep = QDir(_resDir, "morphos.*");
    QStringList ltr = rep.entryList();
    ltr.removeOne("morphos.la");  // S'il traine encore...
    foreach (QString nfl, ltr)
        lisMorphos(QFileInfo(nfl).suffix());
    lisModeles();
    lisLexique();
    lisTags(false);
    lisTraductions(true, false);
    lisIrreguliers();
#ifdef VERIF_TRAD
    foreach (Lemme *l, _lemmes.values()) {
        QString t = l->traduction("fr");
        if (t == "") qDebug() << l->cle() << "non traduit.";
    }
#endif
}

/**
 * @brief LemCore::lisTags
 * @param tout : bool
 *
 * Lorsque le booléen tout est false, on ne lit que les nombres d'occurrences des tags.
 *
 * Lorsque le booléen tout est true, on lit tout le fichier,
 * donc aussi les dénombrements des séquences de trois tags.
 *
 * Cette routine lit le fichier tags.la.
 * Ce fichier a été tiré du traitement
 * des textes lemmatisés du LASLA.
 * C'est un csv, avec la virgule comme séparateur.
 *
 * La première partie du fichier donne le nombre d'occurrences de chaque tag
 * que j'ai introduit pour traiter les textes du LASLA.
 * Elle établit aussi la correspondance avec les tags de Collatinus.
 *
 * La deuxième partie donne les séquences de trois tags (LASLA) et
 * le nombre d'occurrences mesuré.
 *
 */
void LemCore::lisTags(bool tout)
{
    // Nouveau format des données. Le 8 novembre 2016.
    _tagOcc.clear();
    _tagTot.clear();
    _trigram.clear();
    QStringList lignes = lignesFichier(_resDir + "tags.la");
    int max = lignes.count() - 1;
    int i = 0;
    QString l = "";
    QStringList eclats;
    while (i <= max) // && !l.startsWith("! --- "))
    {
        l = lignes.at(i);
        if (l.startsWith("! --- ")) break;
        eclats = l.split(',');
        _tagOcc[eclats[0]] += eclats[1].toInt();
        _tagTot[eclats[0].mid(0,1)] += eclats[1].toInt();
        ++i;
    }
    //  qDebug() << _tagOcc.size() << _tagTot.size();
    if (tout)
    {
        l.clear();
        ++i;
        while (i <= max && !l.startsWith("! --- "))
        {
            l = lignes.at(i);
            eclats = l.split(",");
            _trigram.insert(eclats[0],eclats[1].toInt());
            ++i;
        }
        //  qDebug() << _trigram.size();
    }
}

/**
 * @brief LemCore::tag
 * @param l : le pointeur vers le lemme
 * @param morph : l'analyse morphologique
 * @return : le tag pour Collatinus
 *
 * Cette routine calcule le tag correspondant
 * à l'analyse morphologique donnée, morph,
 * pour le lemme, l.
 * Ce tag est toujours sur trois caractères.
 *
 * Ce tag est obtenu avec le POS du lemme,
 * suivi des cas (1-6 ou 7) et nombre (1, 2) pour les formes déclinées.
 * Pour les verbes conjugués, on donne le mode (1-4)
 * et un 1 si c'est un présent ou un espace sinon.
 * Les supins ont été joints aux impératifs autres que le présent (groupes trop peu nombreux).
 * Les formes verbales déclinées ont un "w" en tête (à la place du "v" pour verbe).
 * Pour les invariables, le POS est complété avec deux espaces.
 *
 */
QString LemCore::tag(Lemme *l, int m)
{
    // Il faut encore traiter le cas des pos multiples
    QString lp = l->pos();
    if ((lp.size() > 0) && !lp[0].isLetter()) lp = "";
//    qDebug() << lp << lp.size();
    QString lTags = "";
    QString morph = morpho(m);
    while (lp.size() > 0)
    {
        QString p = lp.mid(0,1);
        lp = lp.mid(1);
        if ((p == "n") && (m == 413)) // Locatif !
            lTags.append("n71,");
        else if ((p == "v") && (morph.contains(" -u"))) // C'est un supin
            lTags.append("v3 ,");
        else if (!p.isEmpty())
        {
            p.append("%1%2,");
            if (p.startsWith("v"))
            {
                for (int i=0; i<4; i++) if (morph.contains(modes(i).toLower()))
                {
                    if (morph.contains(temps(0))) p = p.arg(i+1).arg(1); // présent
                    else  p = p.arg(i+1).arg(" ");
                    lTags.append(p);
                    break;
                }
            }
            if (p.size() > 4) // Si p == 4, c'est que c'était un verbe conjugué.
            {
                for (int i=0; i<6; i++) if (morph.contains(cas(i)))
                {
                    if (morph.contains(nombre(1))) p = p.arg(i+1).arg(2);
                    else  p = p.arg(i+1).arg(1);
                    if (p.startsWith("v")) p[0] = 'w'; // Forme verbale déclinée.
                    lTags.append(p);
                    break;
                }
            }
            if (p.size() > 4)
            {
                p = p.arg(" ").arg(" ");
                lTags.append(p);
            }
            //if (!_tagOcc.contains(p.mid(0,3)))
            //   qDebug() << l->cle() << morph << p << " : Tag non trouvé !";
        }
    }
    return lTags;
}

/**
 * @brief LemCore::fraction
 * @param t : le tag
 * @return : la fraction moyenne du tag.
 *
 * Ce résultat est un entier, exprimé en 1/1024e
 *
 * On va chercher le nombre d'occurrences associé à ce tag.
 * On le divise par le nombre d'occurrences associé au même POS.
 *
 * Si la fonction reçoit une liste de tags,
 * elle retourne la plus grande fraction.
 *
 */
int LemCore::fraction(QString listTags)
{
    int frFin = 0;
    while (listTags.size() > 2)
    {
        QString t = listTags.mid(0,3);
        listTags = listTags.mid(4);
        int fr = 0;
        if (_tagOcc.contains(t))
        {
            if ((t[0] == 'a') || (t[0] == 'p') || (t[0] == 'w')) // Adj. ou pron. sans genre !
                fr = _tagOcc[t] * 341 / _tagTot[t.mid(0,1)];
            else if ((t[0] == 'v') && (t[2] == '1')) // verbe au présent
                fr = _tagOcc[t] * 512 / _tagTot[t.mid(0,1)];
            else if ((t[0] == 'v') && (t[2] == ' ')) // verbe à un autre temps
                fr = _tagOcc[t] * 256 / _tagTot[t.mid(0,1)];
            else if (t[0] == 'n') // Nom
                fr = _tagOcc[t] * 1024 / _tagTot[t.mid(0,1)];
            else fr = 1024;
            if (fr == 0) fr = 1;
            //  qDebug() << _tagOcc[t] << _tagTot[t.mid(0,1)] << fr;
        }
        //else qDebug() << t << " : Tag non trouvé !";
        if (frFin < fr) frFin = fr; // Si j'ai reçu une liste de tags, je garde la fraction la plus grande.
    }
    if (frFin == 0) return 1024;
    return frFin;
}

/**
 * @brief LemCore::tagOcc
 * @param t : tag
 * @return Le nombre d'occurrences du tag t
 */
int LemCore::tagOcc(QString t)
{
    return _tagOcc[t];
}

/**
 * @brief LemCore::trigram
 * @param t : seq
 * @return Le nombre d'occurrences du trigram seq
 */
int LemCore::trigram(QString seq)
{
    if (_trigram.isEmpty()) lisTags(true);
    // Si je n'ai pas encore chargé les trigrammes, je dois le faire maintenant.

    return _trigram[seq];
}

/**
 * @brief LemCore::lignesFichier
 * @param nf : nom du fichier
 * @return : l'ensemble de lignes du fichier qui ne sont
 * ni vides ni commentées.
 *
 * Les fichiers de Collatinus ont adopté le point d'exclamation
 * en début de ligne pour introduire un commentaire.
 * Ces lignes doivent être ignorées par le programme.
 *
 */
QStringList LemCore::lignesFichier(QString nf)
{
    QFile f(nf);
    f.open(QFile::ReadOnly);
    QTextStream flux(&f);
    flux.setCodec("UTF-8"); // Pour windôze !
    QStringList retour;
    while (!flux.atEnd())
    {
        QString lin = flux.readLine();
        if ((!lin.isEmpty()) && ((!lin.startsWith("!")) || lin.startsWith("! --- ")))
            retour.append(lin);
    }
    f.close();
    return retour;
}

/**
 * @brief LemCore::lisMorphos
 * @param lang : langue pour les morphologies.
 * Cette langue est donnée par deux caractères "fr" ou "en",
 * pour l'instant.
 *
 * Cette routine lit le fichier morphos.* qui donne
 * les analyses morphologiques en français ou en anglais.
 * Les utilisateurs peuvent ajouter toutes les langues qu'ils maîtrisent.
 *
 * Des mots clefs essentiels sont aussi ajoutés après les 416 morphos possibles.
 *
 */
void LemCore::lisMorphos(QString lang)
{
    QStringList lignes = lignesFichier(_resDir + "morphos." + lang);
    int max = lignes.count() - 1;
    int i = 0;
    QString l; // = "";
    QStringList morphos;
    while (i <= max) // && !l.startsWith("! --- "))
    {
        l = lignes.at(i);
        if (l.startsWith("! --- ")) break;
        if (i+1 != l.section(':',0,0).toInt())
            qDebug() <<i<<"Fichier morphos." << lang << ", erreur dans la ligne "<<l;
        else morphos.append(l.section(':',1,1));
        ++i;
    }
    _morphos.insert(lang,morphos);
    QStringList cas;
    l.clear();
    ++i;
    while (i <= max && !l.startsWith("! --- "))
    {
        l = lignes.at(i);
        cas << l;
        ++i;
    }
    _cas.insert(lang,cas);
    QStringList genres;
    l.clear();
    while (i <= max && !l.startsWith("! --- "))
    {
        l = lignes.at(i);
        genres << l;
        ++i;
    }
    _genres.insert(lang,genres);
    QStringList nombres;
    l = "";
    while (i <= max && !l.startsWith("! --- "))
    {
        l = lignes.at(i);
        nombres << l;
        ++i;
    }
    _nombres.insert(lang,nombres);
    QStringList temps;
    l.clear();
    while (i <= max && !l.startsWith("! --- "))
    {
        l = lignes.at(i);
        temps << l;
        ++i;
    }
    _temps.insert(lang,temps);
    QStringList modes;
    l.clear();
    while (i <= max && !l.startsWith("! --- "))
    {
        l = lignes.at(i);
        modes << l;
        ++i;
    }
    _modes.insert(lang,modes);
    QStringList voix;
    l.clear();
    while (i <= max && !l.startsWith("! --- "))
    {
        l = lignes.at(i);
        voix << l;
        ++i;
    }
    _voix.insert(lang,voix);
    QStringList mc;
    l.clear();
    while (i <= max && !l.startsWith("! --- "))
    {
        l = lignes.at(i);
        mc << l;
        ++i;
    }
    _motsClefs.insert(lang,mc);
}

/**
* \fn void LemCore::ajAssims ()
* \brief définit les débuts de mots
* non assimilés, et associe à chacun sa
* forme assimilée.
*/
void LemCore::ajAssims()
{
    // peupler la QMap assims
    QStringList lignes = lignesFichier(_resDir + "assimilations.la");
    foreach (QString lin, lignes)
    {
        QStringList liste = lin.split(':');
        assimsq.insert(liste.at(0), liste.at(1));
        assims.insert(Ch::atone(liste.at(0)), Ch::atone(liste.at(1)));
    }
}

void LemCore::ajAbrev()
{
    // peupler la QStringList abr
    abr = lignesFichier(_resDir + "abreviations.la");
}

bool LemCore::estAbr(QString m)
{
    return abr.contains(m);
}


/**
 * \fn void LemCore::ajContractions ()
 * \brief Établit une liste qui donne, pour chaque
 * contraction, la forme non contracte qui lui
 * correspond.
 */
void LemCore::ajContractions()
{
    // peupler la QMap _contractions
    QStringList lignes = lignesFichier(_resDir + "contractions.la");
    foreach (QString lin, lignes)
    {
        QStringList liste = lin.split(':');
        _contractions.insert(liste.at(0), liste.at(1));
    }
}

int LemCore::aRomano(QString f)
{
    if (f.size () == 0) return 0;
    // création de la table de conversion : pourrait être créée ailleurs.
    QMap<QChar,int> conversion;
    conversion['I']=1;
    conversion['V']=5;
    conversion['X']=10;
    conversion['L']=50;
    conversion['C']=100;
    conversion['D']=500;
    conversion['M']=1000;
    // calcul du résultat : ajout d'un terme si l'ordre est normal, soustraction sinon.
    int res=0;
    int conv_c;
    int conv_s = conversion[f[0]];
    for (int i = 0; i < f.count()-1; i++)
    {
        conv_c = conv_s;
        conv_s = conversion[f[i+1]];
        if (conv_c < conv_s)
            res -= conv_c;
        else res += conv_c;
    }
    res += conv_s;
    return res;
}

/**
 * \fn void LemCore::ajDesinence (Desinence *d)
 * \brief ajoute la désinence d dans la map des
 * désinences.
 */
void LemCore::ajDesinence(Desinence *d)
{
    _desinences.insert(Ch::deramise(d->gr()), d);
}

bool LemCore::estRomain(QString f)
{
    return !(f.contains(QRegExp ("[^IUXLCDM]"))
             || f.contains("IL")
             || f.contains("IUI"));
}

/**
 * \fn void LemCore::ajRadicaux (Lemme *l)
 * \brief Calcule tous les radicaux du lemme l,
 *  en se servant des modèles, les ajoute à ce lemme,
 *  et ensuite à la map *  des radicaux de la classe Lemmat.
 *
 */
void LemCore::ajRadicaux(Lemme *l)
{
    // ablŭo=ā̆blŭo|lego|ā̆blŭ|ā̆blūt|is, ere, lui, lutum
    //      0        1    2    3         4
    Modele *m = modele(l->grModele());
    /* insérer d'abord les radicaux définis dans lemmes.la
    qui sont prioritaires */
    foreach (int i, l->clesR())
    {
        QList<Radical *> lr = l->radical(i);
        foreach (Radical *r, lr)
            _radicaux.insert(Ch::deramise(r->gr()), r);
    }
    // pour chaque radical du modèle
    foreach (int i, m->clesR())
    {
        if (l->clesR().contains(i)) continue;
        QStringList gs = l->grq().split(',');
        foreach (QString g, gs)
        {
            Radical *r = NULL;
            {
                QString gen = m->genRadical(i);
                // si gen == 'K', le radical est la forme canonique
                if (gen == "-") continue;
                if (gen == "K")
                    r = new Radical(g, i, l);
                else
                {
                    // sinon, appliquer la règle de formation du modèle
                    int oter = gen.section(',', 0, 0).toInt();
                    QString ajouter = gen.section(',', 1, 1);
                    if (g.endsWith(0x0306)) g.chop(1);
                    g.chop(oter);
                    if (ajouter != "0") g.append(ajouter);
                    r = new Radical(g, i, l);
                }
            }
            l->ajRadical(i, r);
            _radicaux.insert(Ch::deramise(r->gr()), r);
        }
    }
}

/**
 * \fn QString LemCore::assim (QString a)
 * \brief Cherche si la chaîne a peut subir
 *        une assimilation, et renvoie
 *        cette chaîne éventuellement assimilée.
 *        *version sans quantités*
 */
QString LemCore::assim(QString a)
{
    foreach (QString d, assims.keys())
        if (a.startsWith(d))
        {
            a.replace(d, assims.value(d));
            return a;
        }
    return a;
}

/**
 * \fn QString LemCore::assimq (QString a)
 * \brief Cherche si la chaîne a peut subir
 *        une assimilation, et renvoie
 *        cette chaîne éventuellement assimilée.
 *        *version avec quantités*
 */
QString LemCore::assimq(QString a)
{
    foreach (QString d, assimsq.keys())
        if (a.startsWith(d))
        {
            a.replace(d, assimsq.value(d));
            return a;
        }
    return a;
}

/**
 * \fn QString LemCore::cible()
 * \brief Renvoie la langue cible dans sa forme
 *        abrégée (fr, en, de, it, etc.).
 */
QString LemCore::cible()
{
    return _cible;
}
/**
 * \fn void LemCore::setCible(QString c)
 * \brief Permet de changer la langue cible.
 */
void LemCore::setCible(QString c) { _cible = c; }

/**
 * \fn QMap<QString,QString> LemCore::cibles()
 * \brief Renvoie la map des langues cibles.
 *
 */
QMap<QString, QString> LemCore::cibles()
{
    return _cibles;
}

/**
 * \fn QString LemCore::decontracte (QString d)
 * \brief Essaie de remplacer la contractions de d
 *        par sa forme entière, et renvoie le résultat.
 */
QString LemCore::decontracte(QString d)
{
    foreach (QString cle, _contractions.keys())
    {
        if (d.endsWith(cle))
        {
            d.chop(cle.length());
            if ((d.contains("v") || d.contains("V")))
                d.append(_contractions.value(cle));
            else
                d.append(Ch::deramise(_contractions.value(cle)));
            return d;
        }
    }
    return d;
}

/**
 * \fn QString LemCore::desassim (QString a)
 * \brief Essaie de remplacer l'assimilation de a
 *        par sa forme non assimilée, et renvoie
 *        le résultat.
 */
QString LemCore::desassim(QString a)
{
    foreach (QString d, assims.values())
    {
        if (a.startsWith(d))
        {
            a.replace(d, assims.key(d));
            return a;
        }
    }
    return a;
}

/**
 * \fn QString LemCore::desassimq (QString a)
 * \brief Essaie de remplacer l'assimilation de a
 *        par sa forme non assimilée, et renvoie
 *        le résultat.
 */
QString LemCore::desassimq(QString a)
{
    foreach (QString d, assimsq.values())
    {
        if (a.startsWith(d))
        {
            a.replace(d, assimsq.key(d));
            return a;
        }
    }
    return a;
}

/**
 * \fn MapLem LemCore::lemmatise (QString f)
 * \brief Le cœur du lemmatiseur
 *
 *  renvoie une QMap<Lemme*,QStringlist> contenant
 *  - la liste de tous les lemmes pouvant donner
 *    la forme f;
 *  - pour chacun de ces lemmes la QStringList des morphologies
 *    correspondant à la forme.
 */
MapLem LemCore::lemmatise(QString f)
{
    MapLem result;
    if (f.isEmpty()) return result;
    QString f_lower = f.toLower();
    int cnt_v = f_lower.count("v");
    bool V_maj = f[0] == 'V';
    int cnt_ae = f_lower.count("æ");
    int cnt_oe = f_lower.count("œ");
    if (f_lower.endsWith("æ")) cnt_ae -= 1;
    f = Ch::deramise(f);
    // formes irrégulières
    QList<Irreg *> lirr = _irregs.values(f);
    foreach (Irreg *irr, lirr)
    {
        foreach (int m, irr->morphos())
        {
            SLem sl = {irr->grq(), m, ""};
            // result[irr->lemme()].prepend (morpho (m));
            result[irr->lemme()].prepend(sl);
        }
    }
    // radical + désinence
    for (int i = 0; i <= f.length(); ++i)
    {
        QString r = f.left(i);
        QString d = f.mid(i);
        QList<Desinence *> ldes = _desinences.values(d);
        if (ldes.empty()) continue;
        // Je regarde d'abord si d est une désinence possible,
        // car il y a moins de désinences que de radicaux.
        // Je fais la recherche sur les radicaux seulement si la désinence existe.
        QList<Radical *> lrad = _radicaux.values(r);
        // ii noté ī
        // 1. Patauium, gén. Pataui : Patau.i -> Patau+i.i
        // 2. conubium, ablP conubis : conubi.s -> conubi.i+s
        if (d.startsWith('i') && !d.startsWith("ii") && !r.endsWith('i'))
            lrad << _radicaux.values(r + "i");
        if (lrad.empty()) continue;
        // Il n'y a rien à faire si le radical n'existe pas.
        foreach (Radical *rad, lrad)
        {
            Lemme *l = rad->lemme();
            foreach (Desinence *des, ldes)
            {
                if (des->modele() == l->modele() &&
                    des->numRad() == rad->numRad() &&
                    !l->estIrregExcl(des->morphoNum()))
                {
                    bool c = ((cnt_v==0)
                              ||(cnt_v == rad->grq().toLower().count("v")
                                 +des->grq().count("v")));
                    if (!c) c = (V_maj && (rad->gr()[0] == 'U')
                            && (cnt_v - 1 == rad->grq().toLower().count("v")));
                    c = c && ((cnt_oe==0)||(cnt_oe == rad->grq().toLower().count("ōe")));
                    c = c && ((cnt_ae==0)||
                              (cnt_ae == (rad->grq().toLower().count("āe") + rad->grq().toLower().count("prăe"))));
                    if (c)
                    {
                        QString fq = rad->grq() + des->grq();
                        if (!r.endsWith("i") && rad->gr().endsWith("i"))
                            fq = rad->grq().left(rad->grq().size()-1) + "ī"
                                    + des->grq().right(des->grq().size()-1);
                        SLem sl = {fq, des->morphoNum(), ""};
                        result[l].prepend(sl);
                    }
                }
            }
        }
    }
    if (_extLoaded && !_extension && !result.isEmpty())
    {
        // L'extension est chargée mais je ne veux voir les solutions qui en viennent que si toutes en viennent.
        MapLem res;
        foreach (Lemme *l, result.keys())
        {
            if (l->origin() == 0)
                res[l] = result[l];
        }

        if (!res.isEmpty()) result = res;
    }
    // romains
    if (estRomain(f) && !_lemmes.contains(f))
    {
        QString f1 = f;
        f.replace('U','V');
        QString lin = QString("%1|inv|||adj. num.|1").arg(f);
        Lemme *romain = new Lemme(lin, 0, this);
        int nr = aRomano(f);
        romain->ajTrad(QString("%1").arg(nr), "fr");
        _lemmes.insert(f1, romain);
        SLem sl = {f1,416,""};
        QList<SLem> lsl;
        lsl.append(sl);
        result.insert(romain, lsl);
    }
    return result;
}

/**
 * \fn bool LemCore::inv (Lemme *l, const MapLem ml)
 * \brief Renvoie true si le lemme l faisant partie
 *        de la MaplLem ml est invariable.
 */
bool LemCore::inv(Lemme *l, const MapLem ml)
{
    return ml.value(l).at(0).morpho == 416;
}

/**
 * @brief LemCore::lemmatiseM renvoie dans une MapLem les lemmatisations de la
 *        forme f. Le paramètre debPhr à true indique qu'il
 *        s'agit d'un début de phrase.
 * @param f : la forme qui s'agit de lemmatiser.
 * @param debPhr : booléen qui indique que l'on est en début de phrase
 * @param etape : initialement 0, permet de suivre un protocole d'étapes
 * @return une MapLem avec toutes les lemmatisations de la forme f.
 *
 * Cette routine est récursive et son but est de lemmatiser la forme f
 * en tenant compte des modifications possibles.
 * Les transformations de la forme peuvent être :
 *     - la contraction amavisse ——> amasse
 *     - l'assimilation du préfixe ads- ——> ass-
 *     - la majuscule initiale en début de phrase ou de vers
 *     - l'ajout d'un (ou plusieurs) suffixe(s) ou enclitique
 *     - la disparition erronée d'une majuscule à un nom propre.
 *
 * Bien que certaines combinaisons ne soient pas attestées,
 * nous n'avons pas voulu les exclure.
 * La structure récursive avec un appel direct à l'étape suivante
 * et un autre appel éventuel après transformation de la forme
 * permet d'explorer toutes les possibilités.
 * Essayées une fois et une seule.
 *
 * Cette routine est a priori sensible à la casse.
 * Ainsi, à l'intérieur d'une phrase (i.e. lorsque debPhr est false),
 * elle distinguera Aeneas (Énée) et aeneas (de bronze).
 * En début de phrase (i.e. lorsque debPhr est true),
 * la majuscule perd cette caractéristique distinctive
 * et Aeneas sera lemmatisé avec ses deux solutions, Énée et d'airain.
 *
 * Dans la constitution du lexique, nous n'avons pas adopté un parti pris
 * pour l'assimilation des préfixes.
 * D'ailleurs, les différents dictionnaires ont des conventions différentes.
 * Ainsi, le Gaffiot fait le renvoi aff ——> adf (p. 83 de l'edition de 1934) :
 * le préfixe n'étant pas assimilé, il faudra chercher adfaber.
 * En revanche, le Lewis & Short pratique l'assimilation et donnera affaber.
 * Cette ambiguité de la forme canonique nous a conduit à essayer
 * l'assimilation (adf ——> aff) et la "déassimilation" (aff ——> adf)
 * de façon systématique sur toutes les formes.
 * Cela mène à quelques fausses lemmatisations. Par exemple, la forme
 * assum peut être un rôti ou la forme assimilée du verbe adsum.
 * En revanche, adsum ne semble pas pouvoir être un rôti.
 *
 * La recherche d'un suffixe (ou enclitique) n'a lieu que si la forme
 * complète n'a pas pu être lemmatisée.
 * Cela évite une lemmatisation hasardeuse et improbable de
 * "mentione" en "mentio"+"ne".
 */
MapLem LemCore::lemmatiseM(QString f, bool debPhr, int etape)
{
    MapLem mm;
    if (f.isEmpty()) return mm;
    if ((etape > 3) || (etape <0)) // Condition terminale
    {
        mm = lemmatise(f);
//        qDebug() << f << etape;
        if (debPhr && f.at(0).isUpper())
        {
            QString nf = f.toLower();
            MapLem nmm = lemmatiseM(nf);
            foreach (Lemme *nl, nmm.keys())
                mm.insert(nl, nmm.value(nl));
        }
        return mm;
    }
    // Si j'arrive ici, c'est que j'ai encore des choses à essayer.
    mm = lemmatiseM(f, debPhr, etape + 1);
    // J'essaie d'abord l'étape suivante
    QString fd; // On ne peut pas créer une variable QString à l'intérieur d'un switch.
    switch (etape)
    { // ensuite diverses manipulations sur la forme
    case 3:
        // contractions
        fd = f;
        foreach (QString cle, _contractions.keys())
            if (fd.endsWith(cle))
            {
                fd.chop(cle.length());
                if ((fd.contains("v") || fd.contains("V")))
                    fd.append(_contractions.value(cle));
                else
                    fd.append(Ch::deramise(_contractions.value(cle)));
                MapLem nmm = lemmatiseM(fd, debPhr, 4);
                foreach (Lemme *nl, nmm.keys())
                {
                    int diff = _contractions.value(cle).size() - cle.size();
                    // nombre de lettres que je dois supprimer
                    for (int i = 0; i < nmm[nl].count(); ++i)
                    {
                        int position = f.size() - cle.size() + 1;
                        // position de la 1ère lettre à supprimer
                        if (fd.size() != nmm[nl][i].grq.size())
                        {
                            // il y a une (ou des) voyelle(s) commune(s)
                            QString debut = nmm[nl][i].grq.left(position + 2);
                            position += debut.count("\u0306"); // Faut-il vérifier que je suis sur le "v".
                        }
                        nmm[nl][i].grq = nmm[nl][i].grq.remove(position, diff);
                    }
                    mm.insert(nl, nmm.value(nl));
                }
            }
        break;
    case 2:
        // Assimilation du préfixe
        fd = assim(f);
        if (fd != f)
        {
            MapLem nmm = lemmatiseM(fd, debPhr, 3);
            // désassimiler les résultats
            foreach (Lemme *nl, nmm.keys())
            {
                for (int i = 0; i < nmm[nl].count(); ++i)
                    nmm[nl][i].grq = desassimq(nmm[nl][i].grq);
                mm.insert(nl, nmm.value(nl));
            }
            return mm;
        }
        fd = desassim(f);
        if (fd != f)
        {
            MapLem nmm = lemmatiseM(fd, debPhr, 3);
            foreach (Lemme *nl, nmm.keys())
            {
                for (int i = 0; i < nmm[nl].count(); ++i)
                    nmm[nl][i].grq = assimq(nmm[nl][i].grq);
                mm.insert(nl, nmm.value(nl));
            }
            return mm;
        }
        break;
    case 1:
        // suffixes
        if (mm.isEmpty())
            // Je ne cherche une solution sufixée que si la forme entière
            // n'a pas été lemmatisée.
        foreach (QString suf, suffixes.keys())
        {
            if (mm.empty() && f.endsWith(suf))
            {
                QString sf = f;
                sf.chop(suf.length());
                // TODO : aequeque est la seule occurrence
                // de -queque dans le corpus classique
                mm = lemmatiseM(sf, debPhr, 1);
                // L'appel est récursif car je peux avoir (rarement) plusieurs suffixes.
                // L'exemple que j'ai trouvé au LASLA est "modoquest".
                bool sst = false;
                if (mm.isEmpty() && (suf == "st"))
                { // Ce test mm.isEmpty() empêche la lemmatisation d'amatust
                    // comme "amatus"+"st".
                    sf += "s";
                    mm = lemmatiseM(sf, debPhr, 1);
                    sst = true;
                }
                foreach (Lemme *l, mm.keys())
                {
                    QList<SLem> ls = mm.value(l);
                    for (int i = 0; i < ls.count(); ++i)
                        if (sst) mm[l][i].sufq = "t";
                        else mm[l][i].sufq += suffixes.value(suf); // Pour modoquest
                    // TODO : corriger la longueur de la dernière voyelle si le suffixe est st.
                    // Attention, elle peut être dans sufq, s'il n'est pas vide, ou dans grq.
                }
            }
        }
        break;
    case 0:
        // Pour les sauvages qui auraient ôté la majuscule initiale des noms propres.
        if (mm.empty() && f[0].isLower())
        { // À faire seulement si je n'ai pas de solution.
            f[0] = f.at(0).toUpper();
            return lemmatiseM(f, false, 1);
            // Il n'est pas utile d'enlever la majuscule que je viens de mettre
        }
        break;
    default:
        break;
    }
    return mm;
}


/**
 * \fn Lemme* LemCore::lemme (QString l)
 * \brief cherche dans la liste des lemmes le lemme
 *        dont la clé est l, et retourne le résultat.
 */
Lemme *LemCore::lemme(QString l) { return _lemmes.value(l); }
/**
 * \fn QStringList LemCore::lemmes (MapLem lm)
 * \brief renvoie la liste des graphies des lemmes
 *        de la MapLem lm sans signes diacritiques.
 */
QStringList LemCore::lemmes(MapLem lm)
{
    QStringList res;
    foreach (Lemme *l, lm.keys())
        res.append(l->gr());
    return res;
}

/**
 * \fn void LemCore::lisIrreguliers()
 * \brief Chargement des formes irrégulières
 *        du fichier data/irregs.la
 */
void LemCore::lisIrreguliers()
{
    QStringList lignes = lignesFichier(_resDir + "irregs.la");
    foreach (QString lin, lignes)
    {
        Irreg *irr = new Irreg(lin, this);
        if (irr != 0 && irr->lemme() != 0)
            _irregs.insert(Ch::deramise(irr->gr()), irr);
#ifdef DEBOG
        else
            std::cerr << "Irréguliers, erreur dans la ligne" << qPrintable(lin);
#endif
    }
    // ajouter les irréguliers aux lemmes
    foreach (Irreg *ir, _irregs)
        ir->lemme()->ajIrreg(ir);
}

/**
 * \fn void LemCore::lisFichierLexique(filepath)
 * \brief Lecture des lemmes, synthèse et enregistrement
 *        de leurs radicaux
 */
void LemCore::lisFichierLexique(QString filepath)
{
    int orig = 0;
    if (filepath.endsWith("ext.la")) orig = 1;
    QStringList lignes = lignesFichier(filepath);
    foreach (QString lin, lignes)
    {
        Lemme *l = new Lemme(lin, orig, this);
        //if (_lemmes.contains(l->cle()))
        //    qDebug() << lin << " existe déjà";
        _lemmes.insert(l->cle(), l);
    }
}

/**
 * \fn void LemCore::lisLexique()
 * \brief Lecture du fichier de lemmes de base
 */
void LemCore::lisLexique()
{
    lisFichierLexique(_resDir + "lemmes.la");
}

/**
 * \fn void LemCore::lisExtension()
 * \brief Lecture du fichier d'extension
 */
void LemCore::lisExtension()
{
//    if (_nbrLoaded) foreach(Lemme *l, _lemmes.values())
  //      l->clearOcc();
    // Si les nombres d'occurrences ont été chargés, je dois les ré-initialiser.
    //qDebug() << "lecture extension";
    lisFichierLexique(_resDir + "lem_ext.la");
//    lisNombres();
}

/**
 * \fn void LemCore::lisModeles()
 * \brief Lecture des modèles, synthèse et enregistrement
 *        de leurs désinences
 */
void LemCore::lisModeles()
{
    QStringList lignes = lignesFichier(_resDir + "modeles.la");
    int max = lignes.count()-1;
    QStringList sl;
    for (int i=0;i<=max;++i)
    {
        QString l = lignes.at(i);
        if (l.startsWith('$'))
        {
            _variables[l.section('=', 0, 0)] = l.section('=', 1, 1);
            continue;
        }
        QStringList eclats = l.split(":");
        if ((eclats.at(0) == "modele" || i == max) && !sl.empty())
        {
            Modele *m = new Modele(sl, this);
            _modeles.insert(m->gr(), m);
            sl.clear();
        }
        sl.append(l);
    }
}

/**
 * \fn void LemCore::lisTraductions()
 * \brief Lecture des fichiers de traductions
 *        trouvés dans data/, nommés lemmes, avec
 *        un suffixe corresponant à la langue cible
 *        qu'ils fournissent.
 */
void LemCore::lisTraductions(bool base, bool extension)
{
//    QString nrep = _resDir;
    QDir rep;
    if (!base && !extension) return;
    if (base && extension) {
        rep = QDir(_resDir, "lem*.*");
    } else if (base) {
        rep = QDir(_resDir, "lemmes.*");
    } else {
        rep = QDir(_resDir, "lem_ext.*");
    }
    QStringList ltr = rep.entryList();
#ifdef VERIF_TRAD
    qDebug() << ltr;
#endif
    if (base) {
        ltr.removeOne("lemmes.la");  // n'est pas un fichier de traductions
    }
    if (extension) {
        ltr.removeOne("lem_ext.la");  // n'est pas un fichier de traductions
    }
    foreach (QString nfl, ltr)
    {
        // suffixe
        QString suff = QFileInfo(nfl).suffix();
        QStringList lignes = lignesFichier(_resDir + nfl);
        if (base)
        {
            // lire le nom de la langue
            QString lang = lignes.takeFirst();
            //lang = lang.mid(1).simplified();
            _cibles[suff] = lang;
        }

        foreach (QString lin, lignes)
        {
            Lemme *l = lemme(Ch::deramise(lin.section(':', 0, 0)));
            if (l != 0) l->ajTrad(lin.section(':', 1), suff);
#ifdef DEBOG
            else
                qDebug() << nfl << "traduction, erreur dans la ligne" << lin
                         << " clé" << Ch::deramise(lin.section(':', 0, 0));
#endif
        }
    }
}

/**
 * \fn Modele * LemCore::modele (QString m)
 * \brief Renvoie l'objet de la classe Modele dont le nom est m.
 */
Modele *LemCore::modele(QString m) { return _modeles[m]; }
/**
 * \fn QString LemCore::morpho (int m)
 * \brief Renvoie la chaîne de rang m dans la liste des morphologies
 *        donnée par le fichier data/morphos.la
 */
QString LemCore::morpho(int m)
{
    QString l = "fr"; // La langue sélectionnée
    if (_morphos.keys().contains(_cible.mid(0,2))) l = _cible.mid(0,2);
    else if ((_cible.size() > 4) && (_morphos.keys().contains(_cible.mid(3,2))))
        l = _cible.mid(3,2);
    if (m < 0 || m > _morphos[l].count())
        return "morpho, "+QString::number(m)+" : erreur d'indice";
    if (m == _morphos[l].count()) return "-";
    return _morphos[l].at(m - 1);
}

QString LemCore::cas(int m)
{
    QString l = "fr"; // La langue sélectionnée
    if (_cas.keys().contains(_cible.mid(0,2))) l = _cible.mid(0,2);
    else if ((_cible.size() > 4) && (_cas.keys().contains(_cible.mid(3,2))))
        l = _cible.mid(3,2);
    return _cas[l].at(m);
}

QString LemCore::genre(int m)
{
    QString l = "fr"; // La langue sélectionnée
    if (_genres.keys().contains(_cible.mid(0,2))) l = _cible.mid(0,2);
    else if ((_cible.size() > 4) && (_genres.keys().contains(_cible.mid(3,2))))
        l = _cible.mid(3,2);
    return _genres[l].at(m);
}

QString LemCore::nombre(int m)
{
    QString l = "fr"; // La langue sélectionnée
    if (_nombres.keys().contains(_cible.mid(0,2))) l = _cible.mid(0,2);
    else if ((_cible.size() > 4) && (_nombres.keys().contains(_cible.mid(3,2))))
        l = _cible.mid(3,2);
    return _nombres[l].at(m);
}

QString LemCore::temps(int m)
{
    QString l = "fr"; // La langue sélectionnée
    if (_temps.keys().contains(_cible.mid(0,2))) l = _cible.mid(0,2);
    else if ((_cible.size() > 4) && (_temps.keys().contains(_cible.mid(3,2))))
        l = _cible.mid(3,2);
    return _temps[l].at(m);
}

QString LemCore::modes(int m)
{
    QString l = "fr"; // La langue sélectionnée
    if (_modes.keys().contains(_cible.mid(0,2))) l = _cible.mid(0,2);
    else if ((_cible.size() > 4) && (_modes.keys().contains(_cible.mid(3,2))))
        l = _cible.mid(3,2);
    return _modes[l].at(m);
}

QString LemCore::voix(int m)
{
    QString l = "fr"; // La langue sélectionnée
    if (_voix.keys().contains(_cible.mid(0,2))) l = _cible.mid(0,2);
    else if ((_cible.size() > 4) && (_voix.keys().contains(_cible.mid(3,2))))
        l = _cible.mid(3,2);
    return _voix[l].at(m);
}

QString LemCore::motsClefs(int m)
{
    QString l = "fr"; // La langue sélectionnée
    if (_motsClefs.keys().contains(_cible.mid(0,2))) l = _cible.mid(0,2);
    else if ((_cible.size() > 4) && (_motsClefs.keys().contains(_cible.mid(3,2))))
        l = _cible.mid(3,2);
    return _motsClefs[l].at(m);
}

/**
 * \fn bool LemCore::optExtension()
 * \brief Accesseur de l'option extension,
 *        qui permet de charger l'extension.
 */
bool LemCore::optExtension() { return _extension; }
/**
 * \fn QString LemCore::variable (QString v)
 * \brief permet de remplacer la métavariable v
 *        par son contenu. Ces métavariables sont
 *        utilisées par le fichier modeles.la, pour
 *        éviter de répéter des suites de désinences.
 *        Elles sont repérées comme en PHP, par leur
 *        premier caractère $.
 */
QString LemCore::variable(QString v) { return _variables[v]; }

/**
 * @brief LemCore::setExtension
 * @param e : bool
 *
 * Cette routine gère l'extension du lexique.
 * Si le paramètre e est true, l'extension du lexique est active.
 * S'il n'a pas encore été chargé, il l'est.
 *
 * Lors de la lecture des préférences (à l'initialisation),
 * cette routine est appelée.
 * Si on ne charge pas l'extension du lexique,
 * je charge quand même les nombres d'occurrences.
 * Ces nombres seront ré-initialisés si on charge l'extension par la suite.
 *
 */
void LemCore::setExtension(bool e)
{
    _extension = e;
    if (!_extLoaded && e) {
        lisExtension();
        lisTraductions(false,true);
        _extLoaded = true;
    }
//    else if (!_nbrLoaded) lisNombres();
}

/**
 * @brief LemCore::lireHyphen
 * @param fichierHyphen : nom du fichier (avec le chemin absolu)
 * \brief stocke pour tous les lemmes contenus dans le fichier
 * l'information sur la césure étymologique (non-phonétique).
 */
void LemCore::lireHyphen(QString fichierHyphen)
{
    foreach (Lemme *l, _lemmes.values()) l->setHyphen("");
    if (!fichierHyphen.isEmpty())
    {
        QStringList lignes = lignesFichier(fichierHyphen);
        foreach (QString linea, lignes)
        {
            QStringList ecl = linea.split('|');
#ifdef DEBOG
            if (ecl.count() != 2)
            {
                qDebug () << "ligne mal formée" << linea;
                continue;
            }
#endif
            ecl[1].replace('-',Ch::separSyll);
            Lemme *l = lemme(Ch::deramise(ecl[0]));
            if (l!=NULL)
                l->setHyphen(ecl[1]);
#ifdef DEBOG
            else qDebug () << linea << "erreur lireHyphen";
#endif
        }
    }
}


/*               tagueur.h
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

#ifndef TAGUEUR_H
#define TAGUEUR_H

#include "lemCore.h"
#include "mot.h"
#include "ch.h"

class Tagueur : public QObject
{
public:
    Tagueur(QObject *parent = 0, LemCore *l=0, QString cible = "", QString resDir="");
    // Pour le tagger
    QString tagTexte(QString t, int p, bool affTout = true, bool majPert = true, bool affHTML = true);

private:
    LemCore * _lemCore;
    QString _resDir;
};

#endif // TAGUEUR_H

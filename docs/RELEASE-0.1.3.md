# Vision-Pro 0.1.3 — correctifs d’entrée et style

## Correctifs livrés

1. **Pencil / doigts / souris** : priorité à la source en contact. Un geste des doigts suspend le survol et les contacts Pencil concurrents ; un contact Pencil bloque les doigts. La reprise nécessite la fin du geste propriétaire. Les boutons et modificateurs de navigation sont relâchés lors d'une annulation.
2. **Blender** : les translations et rotations tactiles utilisent des déplacements relatifs, sans ancrage absolu du curseur. Windows reçoit maintenant des événements relatifs natifs ; les autres logiciels gardent leur schéma de navigation.
3. **Cadence et ordre** : la voie de mouvement est cadencée à 120 envois/s maximum hors barrières fiables. Les positions absolues intermédiaires sont regroupées ; les déplacements relatifs sont cumulés, puis segmentés si nécessaire. Le mouvement en attente précède les clics/relâchements. Les numéros de séquence sont attribués au point d'envoi, ce qui évite les remises à zéro après recréation d'un encodeur. Le contrôleur n'est plus recréé quand les outils se masquent ou que le style change.
4. **Barre de commandes** : le premier geste sur la zone de réapparition est consommé. La barre apparaît après ce geste ; l'action sur un bouton demande un second geste.
5. **Diagnostic** : compteurs de mouvement, regroupement et erreurs d'envoi ; file réseau, file de chiffrement, âge du mouvement en attente ; historique borné de reconnexions et codes/motifs de fermeture. Le relais journalise la durée et la raison de fermeture et annonce explicitement sa limitation de débit. La protection à 400 trames/s reste active ; le flux normal a été régulé côté client. L'export est disponible depuis la session et l'accueil, y compris après une coupure, et conserve la dernière mesure RTT horodatée.
6. **Style tablette** : fonds Graphite, Bleu nuit, Papier ou couleur personnalisée ; pointillés, quadrillage ou aucun ; couleurs, opacité et espacement ; traînée brumeuse facultative avec couleur, largeur et durée. Les réglages se sauvegardent localement. La traînée utilise un canvas borné à 96 points, sans interaction avec le réseau et avec respect du mouvement réduit. Les réglages sont accessibles depuis l'accueil et la session. L'interface PC existante est conservée ; À propos affiche désormais la version du livrable Windows.

## Validation

- TypeScript et construction de production des interfaces réussis.
- 28 tests JavaScript/TypeScript : protocole 10, tablette 12, serveur 6. Nouveaux scénarios : exclusion des sources, annulation de navigation, maintien des distances relatives, ordre avant relâchement, séquences monotones, rafales nominales à 240 Hz et validation des préférences.
- 47 tests unitaires agent, 14 tests unitaires relais et six tests d'intégration relais réussis. Le dépassement de débit est testé et retourne une fermeture 1008 / rate_limited.
- Test d'interface locale avec session simulée : premier geste de réapparition sans action, contrôleur stable à travers masquage/réapparition, fond modifié immédiatement et style conservé après rechargement.
- Site HTTPS réel avec agent Windows isolé : jumelage, refus puis autorisation, reconnexion après rechargement, styles et export JSON des files/fermetures. Client protocole réel : PC hors ligne, preuve absente, mauvaise identité, appareil inconnu et message chiffré altéré refusés.
- Vérification visuelle en format tablette ; contrôles publics à 375×812, 834×1194 et 1194×834, thèmes clair/sombre et mouvement réduit, sans débordement horizontal.
- Construction NSIS 0.1.3 et démarrage du portable final dans un profil isolé réussis. L'installateur n'a pas été exécuté ; binaires non signés.

## Serveur et livraison

Site : https://vision-pro.34-73-186-88.sslip.io

VM vérifiée : zaalis-free-e2-micro, projet zaalis-504018, identifiant 8497323233124612375. Version courante : `/opt/vision-pro/releases/20261022T170517Z-input013`. Retour arrière principal avant cette livraison : `/opt/vision-pro/releases/20261022T162744Z-pairing011`. Le relais a été redémarré pour sa mise à jour ; l'actualisation finale du site n'a pas nécessité un second redémarrage. Compteurs systemd NRestarts à zéro lors de la vérification. Données utilisateurs, HTTPS et IP conservés.

Archive finale : SHA-256 `D3313F9EAD59B6A2B74C56DACA153FEC432047222038CFEE15E7B0B1296C891C`.

Livrables : `installer/connexion-simple-0.1.3/`, avec setup, portable, instructions et SHA256SUMS.txt. Fermer l'ancienne application avant installation, puis recharger le site sur l'iPad.

## Limites de validation

La comparaison avant/après sur l'iPad physique dans la scène Blender de l'utilisateur reste à faire après installation de la 0.1.3. Les tests automatiques ne prouvent ni un ping nul ni une diminution chiffrée des 504–505 ms signalées. L'affichage « Aller estimé » représente toujours RTT/2, avec une mesure de ping distincte ; il ne mesure pas la latence d'affichage ou l'injection réelle dans Blender. La région du relais n'a pas été modifiée.

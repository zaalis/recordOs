# Vision-Pro 0.1.2 — mini rapport

## Correction

La demande d'autorisation apparaissait dans une bannière Windows derrière la fenêtre du code. La troisième étape « Autorisez votre iPad ici » affiche désormais le nom de l'appareil, le code de comparaison et les boutons « Refuser » / « Associer cet iPad » dans cette même fenêtre. La bannière de fond est masquée pendant ce parcours ; elle reste disponible pour les demandes hors de cette fenêtre.

Une fois l'association acceptée, cette étape affiche la confirmation et un bouton « Terminé ». Les réponses sont liées à l'identifiant exact de la demande ; les doubles clics sont bloqués pendant la réponse. Une demande expirée ne propose plus les boutons et les erreurs sont affichées dans la fenêtre. Une hauteur maximale et un défilement interne gardent les actions accessibles sur une petite fenêtre.

Les anciennes indications de profil de certificat ont également été remplacées par les consignes du parcours Internet.

## Mémorisation

La mémorisation existante est conservée. L'iPad enregistre son identité cryptographique dans IndexedDB, sur l'origine du site, et le PC conserve l'autorisation correspondante. Il ne faut pas recopier de jeton. Un rechargement du navigateur réutilise cette identité pour une reconnexion authentifiée sans nouveau code avec les réglages actuels.

Ce stockage est lié au navigateur et au site. Une suppression des données Safari, une navigation privée, une révocation, une réinitialisation de clé ou l'expiration de confiance peut imposer un nouveau jumelage. La configuration actuelle n'impose pas de confirmation à chaque connexion et expire la confiance après 90 jours d'inactivité.

## Validation

- Vérification TypeScript et construction de production des interfaces réussies.
- Interface Windows testée dans Edge avec pont natif simulé : trois étapes, refus, erreur de confirmation puis nouvel essai, association, confirmation finale, fermeture et absence de bannière cachée. Vérifiée à 1120×760 et 860×620, avec mouvement réduit.
- Site HTTPS réel et agent Windows isolé : jumelage, refus puis approbation, rechargement du navigateur et reconnexion sans code ni nouvelle demande PC. Aucune erreur JavaScript.
- Compilation NSIS Windows 0.1.2 réussie ; exécutable portable final lancé dans un profil isolé, identité de l'agent disponible, puis processus de test arrêté.
- L'installateur n'a pas été exécuté automatiquement. Les binaires sont non signés. La validation manuelle dans Safari sur l'iPad de l'utilisateur reste à effectuer.

## Livraison

Dossier : `installer/connexion-simple-0.1.2/`. Installateur : `Vision-Pro_0.1.2_x64-setup.exe`. Le dossier contient aussi la version portable, les instructions et les sommes SHA-256.

Le site et le relais existants sont compatibles ; aucune mise à jour serveur n'était nécessaire pour cette correction Windows. Les anciens livrables et les données utilisateur sont conservés.

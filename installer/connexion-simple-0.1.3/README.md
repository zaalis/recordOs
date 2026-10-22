# Vision-Pro 0.1.3 — Pencil, Blender et style

## Installation

1. Quittez complètement Vision-Pro, y compris l'icône près de l'horloge.
2. Lancez Vision-Pro_0.1.3_x64-setup.exe, puis ouvrez Vision-Pro. La version affichée dans À propos doit être 0.1.3.
3. Sur l'iPad, rechargez https://vision-pro.34-73-186-88.sslip.io pour récupérer l'interface mise à jour. Si la page était déjà ouverte au moment de la publication, rechargez-la une seconde fois après la mise à jour du cache.
4. Reconnectez l'ordinateur déjà associé. La mémorisation reste en place.

Le fichier vision-pro.exe est la variante portable : utilisez une seule version à la fois. L'installation n'a pas été lancée automatiquement. Les binaires ne sont pas signés Authenticode ; Windows peut signaler un éditeur inconnu. SHA256SUMS.txt contient les empreintes.

## Correctifs

- Priorité exclusive : le Pencil ne déplace plus le pointeur pendant un geste des doigts, et les doigts ne perturbent pas un contact Pencil.
- Déplacement relatif pour la navigation Blender et injection relative native Windows.
- Mouvements cadencés à 120 envois/s maximum hors barrières de clic ; les déplacements relatifs sont cumulés et les clics/relâchements restent ordonnés.
- Séquence d'entrée centralisée ; le masquage des commandes ne recrée plus le contrôleur pendant le geste.
- Le premier toucher de la zone de réapparition révèle la barre ; une seconde action est nécessaire pour utiliser un bouton.

## Style et diagnostic

Dans les réglages de la tablette, depuis l'accueil ou en session : Style permet de choisir fond, couleur, pointillés/quadrillage/aucun, couleur et espacement des repères, opacité et traînée brumeuse facultative (couleur, largeur, durée). Ces préférences restent sur cet iPad.

« Exporter le diagnostic de connexion » relève les compteurs, files, mesures RTT et motifs de fermeture. L'export reste accessible depuis l'accueil après une coupure. Il n'inclut ni frappes ni clés privées. « Aller estimé » signifie RTT divisé par deux, pas une mesure de l'affichage dans Blender.

## Test sur votre scène Blender

1. Pencil seul pendant 30 secondes, puis arrêt du mouvement.
2. Navigation avec deux doigts (translation/pincement) et trois doigts (rotation) pendant 30 secondes.
3. Alternance Pencil/doigts pendant une minute : le Pencil doit attendre la fin du geste des doigts.
4. Masquez puis révélez la barre : ce premier toucher ne doit activer aucune commande.
5. Si une coupure ou un saut persiste, exportez le diagnostic depuis les réglages et notez le geste et la valeur de Ping.

Les scénarios ont été testés de façon automatisée. Le contrôle final dans Safari sur votre iPad physique reste à effectuer ; aucune diminution chiffrée de la latence réelle dans votre scène n'est encore revendiquée.

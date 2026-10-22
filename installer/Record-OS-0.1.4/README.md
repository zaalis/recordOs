# Record OS 0.1.4

Lancez **Record-OS_0.1.4_x64-setup.exe** pour installer ou mettre à jour l’application. Quittez d’abord l’application PC si elle est ouverte.

L’installateur personnalisé est en français, avec une interface native Windows adaptée aux écrans à haute densité. Il vérifie l’intégrité de son contenu avant installation et contrôle la version installée avant d’annoncer la réussite. Une installation existante est mise à jour à son emplacement actuel, avec conservation de l’identité, des réglages et des appareils associés.

Le logo d’origine est conservé. L’ancien nom reste uniquement dans certains identifiants internes, emplacements existants et éléments du protocole pour maintenir la compatibilité avec les appareils déjà associés.

**Record OS.exe** est la version portable correspondante. Préférez l’installateur pour l’usage normal. Ne lancez pas deux versions à la fois.

Windows 10/11 en 64 bits requis. Si WebView2 est absent, le moteur standard d’installation peut le télécharger. Le programme n’est pas signé avec un certificat de signature de code ; Windows peut donc afficher une demande de vérification de l’éditeur.

Les fichiers `screenshots/01-bienvenue.png` et `screenshots/02-personnalisation.png` sont des captures du rendu des vrais contrôles WPF de l’installateur, à 150 %, en mode de capture sans installation. Ils ne simulent pas une installation réussie.

Les empreintes des exécutables sont dans **SHA256SUMS.txt**.

Pour reconstruire : `npm run package:installer` à la racine du dépôt.

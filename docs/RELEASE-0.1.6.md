# Record OS 0.1.6 — raccourcis des gestes

19 novembre 2026

Dans **Commandes → Crayon → Apple Pencil et gestes**, activez **Gestes créatifs au doigt**. Le menu **Clic** propose **Clic personnalisé** et le menu **Double tap** propose **Double tap personnalisé**. Chaque geste peut garder son action par défaut ou utiliser sa propre combinaison.

Touchez **Saisir au clavier**, puis tapez par exemple `Ctrl + Z`. Sans clavier physique, choisissez les modificateurs et la touche principale à l’écran. Les deux raccourcis sont enregistrés dans IndexedDB sur l’iPad, pour l’adresse à laquelle Record OS est ouvert. Ils s’appliquent en mode **Crayon** ou **Toucher direct** à l’application Windows au premier plan. La personnalisation concerne les contacts sur l’écran.

## Livraison

- L’interface iPad a été déployée sur le [serveur public Record OS](https://vision-pro.34-73-186-88.sslip.io). Version active `20261119T223522Z-record016`. Le serveur de comptes, le relais et Nginx sont restés actifs pendant le basculement. Les fichiers de l’interface précédente restent disponibles pour les sessions déjà ouvertes.
- Le PC embarque aussi l’interface pour les accès locaux et USB. L’installateur Windows x64 0.1.6 a été recompilé dans `installer/Record-OS-0.1.6/Record-OS_0.1.6_x64-setup.exe`. Le paquet de mise à jour NSIS a été signé et vérifié.
- Les 38 tests et le contrôle TypeScript passent. La saisie clavier, le sélecteur de touches et la restauration des réglages après rechargement ont été vérifiés dans Edge au format mobile. La version servie en HTTPS correspond au bundle embarqué dans Windows. Un iPad physique et une saisie dans un logiciel Windows réel restent à vérifier.

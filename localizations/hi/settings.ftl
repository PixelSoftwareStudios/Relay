# Relay desktop UI catalog, Hindi (hi), section: Settings: this app, hotkey, profile, daemon.
# Terms: call=कॉल, mute=म्यूट, deafen=सुनना बंद, peer=पीयर, channel=चैनल, invite=आमंत्रण, trust=भरोसा, file offer=फ़ाइल प्रस्ताव.
# Same ids and $variables as localizations/en/settings.ftl. See docs/i18n.md.

link-starting = Relay डेमन शुरू हो रहा है
link-connecting = Relay डेमन से जुड़ रहा है
link-connected = जुड़ा हुआ
link-reconnecting = डेमन से कनेक्शन टूटा, फिर जुड़ रहा है
link-offline = डेमन चल नहीं रहा
avatar-size-refusal = प्रोफ़ाइल चित्र 1 बाइट से 1 MiB के बीच होना चाहिए (PNG, JPEG, WebP, GIF, BMP या TIFF)
hotkey-note = यह ऐप कोई ग्लोबल हॉटकी पंजीकृत नहीं करता और अभी इसमें स्क्रीन पर बोलने का बटन नहीं है। पुश-टू-टॉक तभी आवाज़ भेजता है जब इंजन की ओर का इनपुट स्रोत (जैसे CLI) कुंजी दबाए रखे।
close-tray = विंडो बंद करने पर वह ट्रे में छिप जाती है; कॉल चलती रहती हैं।
close-no-tray = इस प्लेटफ़ॉर्म पर ट्रे आइकन नहीं है, इसलिए विंडो बंद करने पर यह ऐप बंद हो जाता है।
quit-note = इस ऐप से बाहर निकलने पर इसके शुरू की या स्वीकार की हुई कॉल समाप्त हो जाती हैं और Relay डेमन चलता रहता है। यह ऐप डेमन को बंद नहीं कर सकता।
calls-active =
    { $count ->
        [one] एक कॉल सक्रिय है।
       *[other] { $count } कॉल सक्रिय हैं।
    }
quit-prompt = इस ऐप से बाहर निकलें?
quit-prompt-calls =
    { $count ->
        [one] इस ऐप से बाहर निकलें? आपकी सक्रिय कॉल समाप्त कर दी जाएगी।
       *[other] इस ऐप से बाहर निकलें? आपकी { $count } सक्रिय कॉल समाप्त कर दी जाएँगी।
    }
daemon-stopped-again = शुरू होते ही यह फिर बंद हो गया।
daemon-keeps-stopping = Relay डेमन बार-बार बंद हो रहा है, इसलिए उसे अपने-आप दोबारा शुरू नहीं किया जाएगा। { $why }
avatar-bad-format = प्रोफ़ाइल चित्र अस्वीकृत: चित्र का प्रारूप असमर्थित या अपरिचित है (PNG, JPEG, WebP, GIF, BMP या TIFF का उपयोग करें)
avatar-zero-size = प्रोफ़ाइल चित्र अस्वीकृत: चित्र की चौड़ाई या ऊँचाई शून्य है
avatar-too-large = प्रोफ़ाइल चित्र अस्वीकृत: चित्र का आकार { $width }x{ $height } है, जो { $limit } पिक्सेल की सीमा से अधिक है
avatar-undecodable = प्रोफ़ाइल चित्र अस्वीकृत: चित्र डिकोड नहीं हो सका: { $error }
daemon-started = Relay डेमन शुरू हो गया।
daemon-already-running = Relay डेमन पहले से चल रहा है।
daemon-not-found = Relay डेमन ({ $binary }) नहीं मिला। इसे इस ऐप के साथ या अपने PATH में इंस्टॉल करें, या { $env } में इसका स्थान बताएँ।
daemon-start-failed = Relay डेमन शुरू नहीं हुआ: { $reason }।
daemon-start-failed-log = Relay डेमन शुरू नहीं हुआ: { $reason }। लॉग की आखिरी पंक्ति: { $last }
daemon-stop-failed = Relay डेमन बंद नहीं हुआ: { $error }
daemon-stopped = Relay डेमन बंद हो गया।
notify-off = बंद: विंडो छिपी या पृष्ठभूमि में होने पर आने वाली कॉल सिर्फ़ ट्रे टूलटिप में दिखती है और डेमन की रिंगटोन से सुनाई देती है।
notify-on = विंडो छिपी हो या सामने न हो तब कॉल आने पर कॉल करने वाले का नाम दिखाता है (संदेश का टेक्स्ट कभी नहीं)।
notify-refused = सिस्टम ने आखिरी सूचना अस्वीकार कर दी ({ $reason })। कॉल फिर भी डेमन के ज़रिए बजती हैं और इस विंडो व ट्रे टूलटिप में दिखती हैं।
login-unsupported = यहाँ उपलब्ध नहीं: { $reason }
login-off = बंद। चालू करने पर यह ऐप सिस्टम में पंजीकृत हो जाता है और लॉगिन पर अपनी विंडो खोलता है। डेमन तब तक शुरू नहीं होता जब तक ऐप को उसकी ज़रूरत न हो।
login-on = चालू: लॉगिन करने पर सिस्टम यह ऐप शुरू करता है (यह अपनी विंडो खोलता है)। प्रविष्टि हटाने के लिए इसे यहाँ बंद करें।
login-failed = लॉगिन आइटम पढ़ा या बदला नहीं जा सका: { $reason }
banner-mock = नकली डेटा: यह दृश्य किसी Relay डेमन से नहीं जुड़ा है।
link-offline-auto = इसे अपने-आप शुरू किया जा रहा है।
daemon-starting-button = डेमन शुरू हो रहा है...
daemon-start-button = डेमन शुरू करें
app-heading = यह ऐप
app-hide = विंडो छिपाएँ
app-quit = इस ऐप से बाहर निकलें (डेमन चलता रहेगा)
app-hang-up-quit = कॉल समाप्त करके बाहर निकलें
autostart-label = डेमन अपने-आप शुरू करें
autostart-hover = डेमन न चल रहा हो तो Relay ऐप उसे शुरू कर देते हैं। ऐप खुला होने पर ही वह स्थानीय नेटवर्क पर अपनी मौजूदगी बताता है।
daemon-heading = Relay डेमन
daemon-not-connected = डेमन से कनेक्शन नहीं है।
daemon-paused = डेमन शुरू करें दबाने तक अपने-आप शुरू होना रुका हुआ है।
daemon-keeps-running = यह ऐप बंद करने पर भी डेमन चलता रहता है। इससे बाहर निकलने पर सभी कॉल समाप्त हो जाती हैं और माइक्रोफ़ोन छूट जाता है।
daemon-quit-ask = डेमन बंद करें...
daemon-quit-confirm = डेमन अभी बंद करें? सक्रिय कॉल समाप्त हो जाएँगी।
daemon-quit = डेमन बंद करें
profile-heading = आपकी प्रोफ़ाइल
profile-name-hint = प्रदर्शित नाम
profile-save-name = नाम सहेजें
profile-image-note = PNG, JPEG, WebP, GIF, BMP या TIFF, 1 MiB और 2048 x 2048 तक। इंजन इसे छोटा करके आपके भरोसेमंद पीयर के साथ साझा करता है। इसे हटाने पर उन्हें इसे दिखाना बंद करने को कहा जाता है।
profile-choose-image = चित्र चुनें…
profile-upload = अपलोड करें
profile-discard = छोड़ दें
profile-remove = चित्र हटाएँ
hotkey-heading = पुश-टू-टॉक हॉटकी
hotkey-intro = कॉल के पुश-टू-टॉक मोड में होने पर बोलने के लिए इस कुंजी को कहीं भी दबाए रखें।
hotkey-field = हॉटकी
hotkey-hint = Ctrl+Alt+Space (खाली रखने पर बंद)
hotkey-delay = छोड़ने में देरी
hotkey-delay-hover = कुंजी छोड़ने के बाद माइक्रोफ़ोन कितनी देर खुला रहता है।
hotkey-nothing = बदलने के लिए कुछ नहीं है।
notify-label = आने वाली कॉल पर सूचित करें
login-label = लॉगिन पर यह ऐप शुरू करें
login-error = लॉगिन पर शुरू करना: { $reason }
dialog-profile-image = प्रोफ़ाइल चित्र चुनें
dialog-image-filter = चित्र
dialog-export = बातचीत निर्यात करें
login-no-mechanism = इस सिस्टम में लॉगिन आइटम की कोई समर्थित व्यवस्था नहीं है
login-no-exe = यह प्रोग्राम नहीं मिल रहा: { $error }
login-bad-path = प्रोग्राम का पथ मान्य टेक्स्ट नहीं है
login-set-unavailable = उपलब्ध नहीं: { $reason }
options-save-failed = चुनाव सहेजा नहीं जा सका: { $error }
language-label = भाषा
language-system = सिस्टम ({ $language })
language-experimental = यह भाषा प्रायोगिक है: टेक्स्ट रेंडरर देवनागरी अक्षरों को जोड़ नहीं पाता, इसलिए कुछ शब्द छपाई से अलग दिखते हैं।
link-offline-manual = डेमन शुरू करें; यह विंडो अपने-आप फिर जुड़ जाएगी।
hotkey-applied = { $label }: अभी लागू हुआ
hotkey-check-accelerator = हॉटकी अधिकतम 64 प्रिंट होने योग्य अक्षरों की हो सकती है
hotkey-check-delay = छोड़ने की देरी 0 से 2000 ms के बीच होनी चाहिए
hotkey-label-accelerator = पुश-टू-टॉक हॉटकी
hotkey-label-delay = छोड़ने में देरी
hotkey-missing-key = डेमन ने { $key } नहीं बताया
hotkey-bad-value = डेमन ने गलत { $key } बताया: { $value }
hotkey-rejected = { $label }: अस्वीकृत
hotkey-rejected-reason = { $label }: अस्वीकृत ({ $reason })
hotkey-saved = { $label }: सहेजा गया, रीस्टार्ट के बाद लागू होगा
hotkey-saved-reason = { $label }: सहेजा गया, रीस्टार्ट के बाद लागू होगा ({ $reason })

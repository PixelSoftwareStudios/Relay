# Relay desktop UI catalog, Hindi (hi), section: Channels, privacy and the connection string.
# Terms: call=कॉल, mute=म्यूट, deafen=सुनना बंद, peer=डिवाइस, channel=चैनल, invite=आमंत्रण, trust=भरोसा, file offer=फ़ाइल प्रस्ताव.
# Same ids and $variables as localizations/en/channels.ftl. See docs/i18n.md.

channels-heading = चैनल
channels-none = आप किसी चैनल में नहीं हैं।
channel-listed = सूचीबद्ध
channel-password = पासवर्ड
channel-invite-only = सिर्फ़ आमंत्रण से
channel-summary = { $role } · { $flags } · { $peers ->
        [one] { $peers } साझा डिवाइस
       *[other] { $peers } साझा डिवाइस
    }
channel-invite = आमंत्रित करें…
channel-leave-yes = हाँ, छोड़ें
channel-leave-stay = रहें
channel-leave = छोड़ें
invite-uses = { $count ->
        [one] { $count } उपयोग
       *[other] { $count } उपयोग
    }
invite-create = आमंत्रण बनाएँ
invite-token-note = आमंत्रण कोड (गोपनीय रखें)। जिसके पास यह है, वह इसकी अवधि खत्म होने या उपयोग पूरे होने तक जुड़ सकता है:
channel-create-heading = चैनल बनाएँ
channel-name-hint = चैनल का नाम
channel-listed-label = सूचीबद्ध: सदस्यों के डिवाइस पर नाम दिखाएँ
channel-password-label = जुड़ने के लिए पासवर्ड ज़रूरी करें
channel-invite-only-label = सिर्फ़ आमंत्रण से
password-hint = पासवर्ड
channel-join-heading = चैनल से जुड़ें
channel-join-note = आमंत्रण कोड चिपकाएँ, या चैनल का नाम या आईडी और उसका पासवर्ड डालें।
channel-join-hint = आमंत्रण कोड, चैनल का नाम या आईडी
password-optional-hint = पासवर्ड (आमंत्रण के लिए ज़रूरी नहीं)
privacy-none = डेमन से जुड़ने के बाद गोपनीयता सेटिंग दिखेंगी।
privacy-inbound-anyone = नेटवर्क पर कोई भी
privacy-inbound-shared = सिर्फ़ मेरे चैनलों के सदस्य
privacy-inbound-nobody = कोई नहीं
privacy-inbound-nobody-help = कोई भी इस डिवाइस पर कॉल नहीं कर सकता। आप फिर भी दूसरों को कॉल कर सकते हैं।
privacy-who-rings = मुझे कौन कॉल कर सकता है
privacy-advert-visible = आस-पास के डिवाइस को दिखता है
privacy-advert-hidden = छिपा हुआ (सिर्फ़ कनेक्शन स्ट्रिंग से पहुँचा जा सकता है)
privacy-visibility = नेटवर्क पर दृश्यता
invite-ttl-hour = 1 घंटा
invite-ttl-day = 1 दिन
invite-ttl-week = 7 दिन
connection-none = Relay को साझा करने के लिए कोई पता नहीं मिला। जाँचें कि वह चल रहा है, फिर दोबारा कोशिश करें।
connection-share = अपने नेटवर्क के दूसरे डिवाइस से कॉल पाने के लिए यह पता साझा करें:
connection-addresses = पते: { $addresses } · पोर्ट { $port }
connection-fingerprint = फ़िंगरप्रिंट { $fingerprint }
connection-warning = इस पते वाला कोई भी आपको कॉल कर सकता है। इससे वह भरोसेमंद नहीं बनता और माइक्रोफ़ोन की पहुँच भी नहीं मिलती।
connection-qr-too-long = पता QR कोड के लिए बहुत लंबा है।
channel-hidden = छिपा हुआ
channels-none-help = चैनल तय करता है कि कौन आपको देख और कॉल कर सकता है, और यह आपका माइक्रोफ़ोन कभी चालू नहीं करता। चैनल बनाएँ या आमंत्रण से शामिल हों।
connection-copy = पता कॉपी करें
connection-error-next = जाँचें कि Relay बैकग्राउंड में चल रहा है, फिर दोबारा कोशिश करें।
connection-qr-help = इस डिवाइस को जोड़ने के लिए दूसरे डिवाइस से यह कोड स्कैन करें।
privacy-who-rings-help = तय करता है कि आपको कौन कॉल कर सकता है।
privacy-visibility-help = छिपे डिवाइस को कनेक्शन स्ट्रिंग से अब भी कॉल किया जा सकता है।
privacy-messages = संदेश
privacy-trusted = भरोसेमंद डिवाइस
privacy-trusted-none = आपने अभी किसी डिवाइस पर भरोसा नहीं किया है।
privacy-trusted-help = किसी डिवाइस पर भरोसा हटाने के लिए उसे भूल जाएँ।
connection-scan = QR कोड स्कैन करें
scan-wrong = यह QR कोड Relay का पता नहीं है।
scan-camera-denied = कोड स्कैन करने के लिए Relay को कैमरा चाहिए। अनुमति दें और फिर कोशिश करें।
scan-camera-blocked = Relay के लिए कैमरे की अनुमति बंद है। स्कैन करने के लिए इसे सिस्टम सेटिंग में चालू करें।
scan-unavailable = यह डिवाइस QR कोड स्कैन नहीं कर सकता। इसके बजाय पता कॉपी करें।
scan-failed = स्कैन नहीं हो सका। फिर कोशिश करें या पता कॉपी करें।
scan-help = दूसरे डिवाइस पर दिखा कोड स्कैन करें, ताकि वह आपके संपर्कों में जुड़ जाए।
scan-trusting = कनेक्ट करके भरोसा किया जा रहा है…
scan-added = { $name } को संपर्कों में जोड़ दिया गया।
scan-added-unnamed = डिवाइस को संपर्कों में जोड़ दिया गया।
scan-unreachable = उस डिवाइस तक नहीं पहुँच सके। जाँचें कि वह उसी नेटवर्क पर है और Relay खुला है, फिर दोबारा स्कैन करें।
scan-mismatch = वह डिवाइस कोड से मेल नहीं खाता, इसलिए उस पर भरोसा नहीं किया गया।
scan-trust-failed = उस डिवाइस को जोड़ा नहीं जा सका: { $reason }
privacy-blocked = ब्लॉक किए गए डिवाइस
privacy-blocked-none = आपने कोई डिवाइस ब्लॉक नहीं किया है।
privacy-blocked-help = ब्लॉक किए गए डिवाइस आपको कॉल नहीं कर सकते और आस-पास नहीं दिखते। अनब्लॉक करने पर वे फिर मिल सकते हैं; वे संपर्क नहीं बन जाते।
nearby-heading = आपके नेटवर्क के चैनल
nearby-devices = { $count ->
        [one] { $count } डिवाइस
       *[other] { $count } डिवाइस
    }
nearby-none = आपके नेटवर्क पर अभी कोई चैनल नहीं मिला
nearby-none-offline = यह डिवाइस ऑफ़लाइन है, इसलिए दूसरे डिवाइस नहीं देख सकता।
nearby-ambiguous = दो चैनल इसी नाम का उपयोग करते हैं। इसके बजाय कोड से जुड़ें।

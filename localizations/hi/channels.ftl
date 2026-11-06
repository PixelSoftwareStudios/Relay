# Relay desktop UI catalog, Hindi (hi), section: Channels, privacy and the connection string.
# Terms: call=कॉल, mute=म्यूट, deafen=सुनना बंद, peer=पीयर, channel=चैनल, invite=आमंत्रण, trust=भरोसा, file offer=फ़ाइल प्रस्ताव.
# Same ids and $variables as localizations/en/channels.ftl. See docs/i18n.md.

channels-heading = चैनल
channels-note = चैनल तय करता है कि कौन आपको देख सकता है और आपको कॉल कर सकता है। यह आपका माइक्रोफ़ोन कभी चालू नहीं करता।
channels-none = आप किसी चैनल में नहीं हैं।
channel-listed = सूचीबद्ध
channel-password = पासवर्ड
channel-invite-only = सिर्फ़ आमंत्रण से
channel-summary = { $role } · { $flags } · { $peers ->
        [one] { $peers } साझा पीयर
       *[other] { $peers } साझा पीयर
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
invite-token-note = आमंत्रण टोकन (गोपनीय)। जिसके पास यह है, वह इसकी अवधि खत्म होने या उपयोग पूरे होने तक जुड़ सकता है:
channel-create-heading = चैनल बनाएँ
channel-name-hint = चैनल का नाम
channel-listed-label = सूचीबद्ध: सदस्यों के डिवाइस पर नाम दिखाएँ
channel-password-label = जुड़ने के लिए पासवर्ड ज़रूरी करें
channel-invite-only-label = सिर्फ़ आमंत्रण से
password-hint = पासवर्ड
channel-join-heading = चैनल में जुड़ें
channel-join-note = आमंत्रण टोकन चिपकाएँ, या चैनल आईडी और उसका पासवर्ड डालें।
channel-join-hint = RLYI1… या चैनल आईडी
password-optional-hint = पासवर्ड (आमंत्रण के लिए ज़रूरी नहीं)
privacy-none = डेमन से जुड़ने के बाद गोपनीयता सेटिंग दिखेंगी।
privacy-inbound-anyone = नेटवर्क पर कोई भी
privacy-inbound-shared = सिर्फ़ मेरे चैनलों के सदस्य
privacy-who-rings = मुझे कौन कॉल कर सकता है
privacy-advert-visible = आस-पास के डिवाइस को दिखता है
privacy-advert-hidden = छिपा हुआ (सिर्फ़ कनेक्शन स्ट्रिंग से पहुँचा जा सकता है)
privacy-visibility = नेटवर्क पर दृश्यता
invite-ttl-hour = 1 घंटा
invite-ttl-day = 1 दिन
invite-ttl-week = 7 दिन
connection-none = डेमन ने कनेक्शन स्ट्रिंग नहीं बताई।
connection-share = अपने नेटवर्क के दूसरे कंप्यूटर से कॉल पाने के लिए यह स्ट्रिंग साझा करें:
connection-addresses = पते: { $addresses } · पोर्ट { $port }
connection-fingerprint = फ़िंगरप्रिंट { $fingerprint }
connection-warning = इस स्ट्रिंग वाला कोई भी आपको कॉल कर सकता है; इससे भरोसा या माइक्रोफ़ोन की पहुँच नहीं मिलती।
connection-qr-too-long = कनेक्शन स्ट्रिंग QR कोड के लिए बहुत लंबी है।
channel-hidden = छिपा हुआ
channels-none-help = कई लोगों से एक साथ बात करने के लिए चैनल बनाएँ, या निमंत्रण से किसी में शामिल हों।
connection-copy = पता कॉपी करें
connection-error-next = जाँचें कि डेमन चल रहा है, फिर दोबारा कोशिश करें।
connection-qr-help = दूसरे डिवाइस से यह कोड स्कैन करें ताकि वह आपको कॉल कर सके।
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
scan-mismatch = वह डिवाइस कोड से मेल नहीं खाता (उसका फ़िंगरप्रिंट अलग है), इसलिए उस पर भरोसा नहीं किया गया।
scan-trust-failed = उस डिवाइस को जोड़ा नहीं जा सका: { $reason }

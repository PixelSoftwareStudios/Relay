# Relay desktop UI catalog, Hindi (hi), section: Tray menu and tooltip.
# Terms: call=कॉल, mute=म्यूट, deafen=सुनना बंद, peer=पीयर, channel=चैनल, invite=आमंत्रण, trust=भरोसा, file offer=फ़ाइल प्रस्ताव.
# Same ids and $variables as localizations/en/tray.ftl. See docs/i18n.md.

tray-mock = Relay (नकली डेटा)
tray-starting = Relay: डेमन शुरू हो रहा है
tray-connecting = Relay: डेमन से जुड़ रहा है
tray-reconnecting = Relay: डेमन से कनेक्शन टूटा, फिर जुड़ रहा है
tray-offline = Relay: डेमन चल नहीं रहा
tray-incoming =
    { $count ->
        [one] Relay: { $count } आने वाली कॉल
       *[other] Relay: { $count } आने वाली कॉल
    }
tray-active =
    { $count ->
        [one] Relay: { $count } सक्रिय कॉल
       *[other] Relay: { $count } सक्रिय कॉल
    }
tray-idle = Relay: निष्क्रिय
tray-show = Relay दिखाएँ
tray-quit = इस ऐप से बाहर निकलें (डेमन चलता रहेगा)
tray-status-available = उपलब्ध
tray-status-unavailable = अनुपलब्ध
tray-stay-available = बैकग्राउंड में उपलब्ध रहें
